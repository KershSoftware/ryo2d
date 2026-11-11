# AGENTS.md - Logging Module

The engine's log: the `log` crate's macros in front, one `log::Log` (the engine's logger) behind them, and its outputs. Named `logging` so that `log::` in engine code always means the `log` crate. The design is `docs/specs/2026-11-09-dev-tools-design.md` ("Logging"; where it and the code differ, the code is current).

| File | What |
|------|------|
| `mod.rs` | The logger, `ensure` (installs it), `set_level`/`level`, the thread's frame (`set_frame`), `installed_by_engine`, `headless_run`, and the exports (public as `ryo2d::logging`) |
| `levels.rs` | `Levels`: a default per origin, overrides by target prefix, `RYO_LOG`'s parser |
| `record.rs` | `LogRecord`, `Origin` (read from the target), `under` (whole-segment prefix match) and `clock` (a record's time as `HH:MM:SS.mmm`) |
| `outputs.rs` | `emit`: the outputs in turn, the sequence counter, `set_stderr` and the stderr line's format |
| `ring.rs` | The ring (`#[cfg(dev_tools)]`): the last `RING` (10,000) records, read with `after(seq, scope, out)`; per scope, the warnings and errors given and dropped; `mirror` (the dev tools' one reader: the newest `limit` of a scope at a level after `seq`, and how many of the scope's warnings and errors it will not show, dropped or past `limit`; the console's mirror reads at its level, the logger window's `LogFeed` at `Trace`), `start` (just before the oldest record) and `may_have_after` (an atomic load, no lock); `after` is the ring's own tests' reader |
| `scope.rs` | Log scopes: the thread's scope, `in_scope`, `scoped` (a spawned thread's body), `new_scope` |
| `capture.rs` | `capture_log`, `capture_log_bounded`, `LogCapture` (re-exported by `ryo2d::testing`) |
| `file.rs` | The log file (native): `open`, `write`, `tick`, `flush`, the records kept before it opens (`drop_early`); `logs/latest.log` in the save folder |
| `panic.rs` | The panic hook: logs `panic at file:line: message` under `ryo2d::panic`, flushes the file, chains to the earlier hook |
| `macros.rs` | `trace!`, `debug!`, `info!`, `warn!`, `error!`, `warn_once!` (`#[macro_export]`, in the prelude) |

## Logging

- **Engine code** logs with `log::trace!` to `log::error!`; the target is the module path. No `println!`/`eprintln!` in engine library code outside `src/cli/`, `src/core/network/` and this module's own stderr output.
- **A game** logs with the prelude's macros, which call `ensure()` and then `log`'s macro with the target `concat!("game::", module_path!())`. So the first line a game logs, before any engine is built, installs the logger and is kept. `warn_once!` keeps one `AtomicBool` per call site and logs the first time the site is reached. A game that calls `log::info!` itself is a library to the log.
- **Origins** by target: `game` or under it is the game; `ryo2d`, under it, or `ryo2d_*` is the engine; anything else (`gamepad` too) is a library.

## Levels

- Defaults: `info` for the game and the engine, `warn` for libraries.
- Overrides are `(prefix, level)`; a target takes the longest prefix it is under, matched by whole path segments (`game` covers `game::menu`, not `gamepad`; `ryo2d` does not cover `ryo2d_procgen`). Else its origin's default.
- Three reserved words name the origin defaults everywhere a level is set or read (`Levels::set` and `read`, so `RYO_LOG`, `set_level`/`level` and the console's `log:level`): `game` (every `game` and `game::…` target), `engine` (every `ryo2d…` and `ryo2d_…` target, which the prefix `ryo2d` alone does not reach: it does not cover `ryo2d_procgen`) and `library` (every other target). Each sets the default, so a nearer override still wins, and there is never an override named after them. A crate whose target is exactly `engine` or `library` cannot be set at its root; its sub-targets (`engine::physics`) can.
- `RYO_LOG` is read once, when the logger installs: comma-separated entries, each a level (sets all three defaults) or `target=level`, trimmed, levels case-insensitive (`off`, `error`, `warn`, `info`, `debug`, `trace`). Order does not matter: the bare level applies first (the last one if there are several), then the `target=level` entries, so `game=debug,warn` and `warn,game=debug` agree. A bad entry is reported once on stderr (`ryo2d: RYO_LOG: ...`) and skipped; the rest applies.
- `log::set_max_level` follows `Levels::most_verbose()` after every change, so at the default levels a `debug!` costs one atomic read in `log`'s macro.
- The logger checks the record's level against its target before formatting; a record that passes is formatted once (`args().as_str()` when it has no arguments) and every output takes that string.

## Installing

`ensure()` installs once per process (a `OnceLock`, safe from any thread; later calls are one atomic read). `EngineBuilder::new` (so the build's warnings reach every output, `tests/builder_log.rs`), `Workspace::discover`, `discover_from` and `from_path` (Ryo.toml's warnings), `Engine::new` (first thing), the prelude's macros and `capture_log` call it. If another `log` logger was set first, it is kept: the engine's outputs stay off, one line says so on stderr, `installed_by_engine()` is false (`set_level` then leaves `log`'s maximum alone), and no log file is opened (it would empty `latest.log` and push the last run's to `previous.log` for nothing). The decision is the pure `outcome`, unit-tested, since a test process can only install once.

## Outputs

`outputs::emit`, in order, after the one format:

1. **The ring** (a build with tools): takes the record's sequence number under its own lock, so the ring is in sequence order, and keeps a copy. Without tools the number comes straight from the counter. Sequence numbers start at 1 and never repeat.
2. **stderr** (native, while on): `LEVEL target: message` with `eprintln!`, so the test harness captures a test thread's lines. A record whose scope has a capture skips it. On from the start in a build with tools, off without; `set_stderr` (crate-internal) switches it. A headless engine turns it on in every build (`headless_run`, from `Engine::new`: a headless run is never a player's and writes no file), and `ryo2d check-ui` turns it off (it prints the mistakes itself). `benches/dev_tools.rs` turns it off through `testing::set_log_stderr`.
3. **The browser console** (wasm, every build): `console.error`, `warn`, `info`, `debug` by level.
4. **The log file** (native, a windowed engine only; a headless engine and a test open none): `HH:MM:SS.mmm LEVEL target: message`, the record's own time, every record whatever its scope. `open(folder)` makes `folder/logs/`, moves `latest.log` to `previous.log`, opens `latest.log`, else `latest-<pid>.log`, else returns the error (`open_log_file` says it once on stderr). An 8 KiB buffer; a warning or error flushes at once unless the last flush was under 50 ms ago, `tick` (each frame) flushes the rest, `flush` runs at exit (the event loop's `exiting`, and again when `Engine::run` returns, for lines logged after the loop's last event) and in the panic hook. After 8 MiB one `log full` line is written and no more. Its mutex is its own, held for its write and flush; a thread holding it that panics skips the file in the hook rather than wait. A failed write detaches the file with one stderr line. Until a file opens, the output keeps the newest 256 records (`EARLY`, its own mutex, held for a push) and writes them first, in order, when one does, so a warning from Ryo.toml or the builder reaches the file; the buffer is bounded whatever happens, and dropped (with no more kept: one atomic read a record after) when an engine starts headless or the file cannot be had. Tests take the file's turn (`file::test_turn`), a test's file takes only its own thread's records, and only a test thread that asked (`keep_early_records`) keeps records for a file or can drop them, so the engine tests' headless engines leave the file tests' buffer alone.
5. **A capture**, when the record's scope has one.

No output holds a lock while another writes: the ring and each capture have their own mutex, held only for their push; the capture registry is read-locked only to find the capture. Per record, besides the level check and the outputs, the cost is the two thread-local reads (scope, frame).

## Scopes and the frame

- A scope is a number a thread holds (0 in a game). `new_scope` never gives 0 or a number twice. `in_scope(scope, f)` sets this thread's scope for `f` and puts the earlier one back, on a panic too.
- `Engine::new` remembers the scope of the thread that builds it (`Engine::log_scope`), and `Engine::frame` runs inside it.
- Every thread the engine spawns runs its body through `scoped(f)`, which reads the scope on the spawning thread: the asset workers (`asset/workers.rs`, one pool per `AssetServer`, started at its first queued load), the asset folder's debounce thread and notify's callback (`asset/watch.rs`, per server; the callback, on notify's own thread, goes through `in_scope`), the shader compiler (`shaders/compile.rs`, per renderer), the save writer (`saves/writer.rs`, per `SaveService`), the tile map job threads (`tilemap/jobs.rs`, one per landing system and per stream) and the capture writer (`capture/writer.rs`, one per capture). Each is owned by its engine and started from the engine's thread, so it holds that engine's scope; none is shared between engines. `network/` keeps its own threads. A new spawn site must go through `scoped`, or its records land in scope 0 and escape a test's capture.
- The frame is the logging thread's (`set_frame`): `Engine::frame` sets it on its own thread at the start of each frame, to the game clock's frame count once the frame has advanced it (1 for the first). A worker's records carry `None`.

## Capture (tests)

- `capture_log()` calls `ensure()`, opens `new_scope()` on its thread for its lifetime and gives back the earlier scope on drop (nested captures unwind in order). It collects every record logged in its scope: the test thread's and those of threads given its scope. It is `!Send`: it must drop on the thread that opened it.
- `capture_log_bounded(limit)` keeps only the newest `limit` records; `seen()` counts every one. For flood tests.
- A capture dropped while its thread panics (a failing test) prints its records on stderr, a line each in the stderr output's format, so the warnings that explain the failure are shown; otherwise captured records never reach stderr.
- Helpers: `records`, `count(level)` (exact level), `warnings` (messages at `Warn`), `warnings_and_errors` (messages at `Warn` or `Error`), `containing(needle)`, `containing_all(needles)`, `from_target(prefix)` (plain `starts_with`), `scope`.
- Nested captures dropped out of order: a capture gives back the earlier scope only while the thread is still in its own, so an outer one dropped first leaves the inner's scope in place.
- **Tests never read stderr, and read the ring only in their capture's scope** (the ring's, the console mirror's and the logger window's feed tests, below). Every test that logs opens a capture first (before any engine, world or asset server, whose threads take the scope they are started in) and asserts on it. A test that changes a level holds `level_for_test`'s guard (or `levels_for_test`'s, which puts every level back), which puts the level back and holds a process-wide lock, so no two level-changing tests overlap (one guard per test).
- **A test never lowers a level default** (`game`, `engine` or `library`): other tests, which hold no guard, would lose the warnings they expect (a library's warning, the ring's flood). Raising a default is allowed only while holding the levels guard, set and restored within the test. Setting a default is covered on a `Levels` value (`levels.rs`'s unit tests), not through the global logger.
- **Once-a-process warnings** keyed through the UI's `warn_once` (`ui/theme/look.rs`) are kept per log scope, so each test's capture sees its own first warning; a game has one scope (0), so for it nothing changes.
- **The ring's own tests** ask for their scope (`after(seq, Some(scope), ..)`) and take turns on a test mutex (`ring_turn`), so a flood cannot push the other's records out. The logger window's feed tests read their capture's scope and take the same turn.
