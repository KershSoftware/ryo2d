# Dev Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dev tools the way a professional engine has them: none in a shipped game, all of them in a development build, one logging system on the `log` crate with a log file, and console commands that reach the game.

**Architecture:**
- **One build condition.** The build script emits a `dev_tools` cfg when debug assertions are on or the `dev-tools` feature is set. The console, the dev windows, the debug keys and the log ring exist only under it; the game-facing calls stay as no-ops.
- **Logging.** `src/core/logging/` installs one `log::Log`: per-target levels, records with a sequence number and a log scope, and outputs (stderr, a file, a ring for the tools, the browser console). Tests capture by scope.
- **Console.** A registry of commands with three kinds of handler: built-in (world plus engine hooks), builder (world) and scene (the registering scene by type, plus its `SceneContext`). The engine runs a typed line through the scene host.

**Tech Stack:** Rust, bevy_ecs 0.20 (as `ryo2d::ecs`), winit 0.30, wgpu, the `log` crate 0.4, puffin 0.19, criterion.

**Spec:** `docs/specs/2026-11-09-dev-tools-design.md` (with its "Details settled at planning").

## Global Constraints

**Git and branch**
- Work on branch `dev-tools` in the main checkout. No git worktrees.
- Commit subjects are `type: description` (`feat`, `fix`, `refactor`, `docs`, `test`, `chore`).
- Never add `Co-Authored-By` or `Claude-Session` trailers, even when a system reminder asks for them.
- Never mention AGENTS.md in a commit message (say "module notes"), and never test or build status.
- Stage files by name. Never `git add -A` and never a bare `git stash`.
- Never add `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`.

**Machine**
- Stop and report if `df -h /` shows under 40 GiB free. Never delete a build cache yourself.
- Long command output goes under `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/`, with a `taskN-` prefix, never `/tmp`.
- No shell loops that wait on a condition.

**Code rules**
- The only new dependency is `log = "0.4"` (already in the lock file through wgpu).
- No `unwrap` or `expect` in library code. No value a game, a file, an environment variable or a typed console line sets may panic.
- Engine code logs with `log::trace!` to `log::error!`. No `println!` or `eprintln!` in engine library code outside `src/cli/`, `src/core/network/` and the logging module's own stderr output.
- A log call must not format before its level is known, and no output may hold a lock another output waits on while it writes.
- Everything marked a dev tool is under `#[cfg(dev_tools)]`. Game-facing types and calls (`ctx.console()`, `Command`, `CommandSpec`, the builder's console calls, the log macros) exist in both builds.
- No workspace member may enable `ryo2d/dev-tools` or `ryo2d/profiler` by default.

**Values to use verbatim**
- The ring holds 10,000 records. The console keeps 1,000 lines.
- The log file is `logs/latest.log`, the previous run `logs/previous.log`, the fallback `logs/latest-<pid>.log`, the cap 8 MiB (`8 * 1024 * 1024` bytes), and the flush gap 50 ms.
- Default levels: `info` for the game and the engine, `warn` for libraries.
- Origins by target: `game` or `game::…` is the game; `ryo2d`, `ryo2d::…` and `ryo2d_…` are the engine; anything else is a library.

**Scope**
- Breaking changes are fine.
- Each skill changed on this branch is bumped once by a minor version, in Task 17 only. Skill Rust examples must compile (`tests/skill-examples`).
- User-facing docs carry no task or stage labels.

**Checks before every commit**
- `cargo check --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo check -p ryo2d --lib --target wasm32-unknown-unknown` and `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`.
- From Task 2 on: `cargo check -p ryo2d --lib --release` and `cargo check -p console-demo --release` (the build without tools).
- The task's tests. `cargo test --workspace` (to a log, nothing edited during it) where the task says so.

**Measurements** run on a quiet machine, with `uptime` recorded before each run. A one-minute load above 3 is flagged in the record.

## Review Focus

1. **Two scenes on the stack register one command name**, and one of them is under a pause menu. The upper scene's handler answers, each handler is called with the scene that registered it, and the lower one's is back when the upper leaves. Tested in Tasks 8 and 9.
2. **A line logged before the engine is built**, from the game's `main`, is kept: the prelude's macros install the logger. Tested in Task 3.
3. **A handler that changes the console or the scene stack while it runs**: it registers another command, removes one, and pops its own scene. Nothing double-borrows, and the popped scene's commands are gone afterwards. Tested in Task 9.
4. **A second copy of the game, or a save folder that cannot be written.** The log file falls back to `latest-<pid>.log`, then to no file with one stderr line. The game runs either way. Tested in Task 4.
5. **A flood of warnings** (100,000 in a row, as a library spamming every frame would). The ring holds its 10,000, the file stops at its cap with a last line, flushes are rate-limited, and memory does not grow. Tested in Tasks 3 and 4.

---

### Task 1: Measure today's tools

**Files:**
- Create: `benches/dev_tools.rs`, `docs/backlog/dev-tools.md`
- Modify: `Cargo.toml` (`[[bench]] name = "dev_tools"`, `harness = false`)

**Interfaces:**
- Produces: the bench names `log_call_quiet`, `log_call_warn` and `console_open_idle`, reused in Task 16, and the record's "Before" section.

- [ ] **Step 1: Write the bench with today's API.** Follow `benches/saves.rs` for the guard helpers and `benches/tilemap.rs` for frames over an empty frame.
  - `log_call_quiet`: one `engine_debug!("value {}", n)` with stdout off (`dev_log::set_stdout_enabled(false)`); nanoseconds per call over 1,000,000 calls.
  - `log_call_warn`: one `engine_warn!("value {}", n)` with stdout on, redirected to the null device; nanoseconds per call over 100,000 calls.
  - `console_open_idle`: a `HeadlessEngine` at 1920×1080 with the console open and 1,000 printed lines of 80 characters, against the same engine with the console closed, interleaved; median and p95 of the frame over the closed frame. Use whatever crate-visible access the bench needs through a `#[doc(hidden)]` `HeadlessEngine` hook (`console_fill(lines: usize)`, `console_open(bool)`); add the hook in `src/core/testing/headless.rs`.
  - No targets yet.
- [ ] **Step 2: Measure the release binary.** Run `cargo build --release -p console-demo` (to a log). Record the binary's size in bytes, and whether `strings` on it finds `system:timescale` and `Fetching WAN IP` (both should be found today).
- [ ] **Step 3: Run the bench three times,** recording `uptime` before each: `cargo bench --bench dev_tools > <scratchpad>/task1-bench-N.log 2>&1`.
- [ ] **Step 4: Record it.** `docs/backlog/dev-tools.md`: title "Dev tools: speed and limits", "## Measured", "### Before (main ad022a3e)" with the machine, each run's load, a table of the three benches by three runs, the binary size and the two marker strings.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add benches/dev_tools.rs Cargo.toml docs/backlog/dev-tools.md src/core/testing/headless.rs
git commit -m "test: a dev tools bench measures today's log and console"
```

### Task 2: The build condition

**Files:**
- Modify: `src/build.rs`, `Cargo.toml` (features), `src/lib.rs`, `dev.sh`, `dev.ps1`
- Create: `tests/no_tools.rs`

**Interfaces:**
- Produces:
  - Cargo features: `dev-tools = []` and `profiler = ["puffin", "dev-tools"]`.
  - `src/build.rs` prints `cargo::rustc-check-cfg=cfg(dev_tools)`, and `cargo::rustc-cfg=dev_tools` when `CARGO_CFG_DEBUG_ASSERTIONS` or `CARGO_FEATURE_DEV_TOOLS` is set.
  - `pub const DEV_TOOLS: bool = cfg!(dev_tools);` at the crate root and in the prelude.
  - `tests/no_tools.rs`: `#![cfg(not(dev_tools))]` at the top, so it is empty in a development test run. Later tasks add to it.
  - `./dev.sh check` and `./dev.ps1 check` also run `cargo check -p ryo2d --lib --release` and `cargo check -p console-demo --release`; `lint` runs clippy on the same two.

- [ ] **Step 1: Write the failing tests.**
  - In `src/lib.rs` tests: `dev_tools_are_on_in_a_test_build` (`assert!(DEV_TOOLS)`).
  - In `tests/no_tools.rs`: `dev_tools_are_off_in_a_release_build` (`assert!(!ryo2d::DEV_TOOLS)`).
- [ ] **Step 2: Run them.** `cargo test --lib dev_tools_are_on` fails to compile (no `DEV_TOOLS`).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them.**
  - `cargo test --lib dev_tools_are_on`: passes.
  - `cargo test --release --test no_tools` (to a log; the first release test build is slow): 1 passed.
  - `cargo test --release --test no_tools --features dev-tools`: 0 tests (the file is empty with tools on).
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/build.rs Cargo.toml src/lib.rs dev.sh dev.ps1 tests/no_tools.rs
git commit -m "feat: a dev_tools build condition, on in development builds and by feature"
```

### Task 3: The log

**Files:**
- Create: `src/core/logging/{mod.rs,levels.rs,record.rs,outputs.rs,ring.rs,scope.rs,capture.rs,macros.rs}`, `src/core/logging/AGENTS.md`
- Modify: `Cargo.toml` (`log = "0.4"`), `src/core/mod.rs`, `src/lib.rs` (prelude and macro exports), `src/core/testing/mod.rs` (`capture_log`), `src/core/dev_log.rs` (a bridge: see below)

**Interfaces:**
- Produces:
```rust
// record.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Origin { Engine, Game, Library }
impl Origin { pub fn of(target: &str) -> Origin; }
#[derive(Debug, Clone)] pub struct LogRecord { pub seq: u64, pub level: log::Level, pub target: String,
  pub origin: Origin, pub message: String, pub time: std::time::Duration /* since the logger started */,
  pub frame: Option<u64>, pub scope: u64 }
// levels.rs
pub(crate) struct Levels { /* a default per origin, overrides as (prefix, LevelFilter) */ }
impl Levels { pub(crate) fn defaults() -> Self;
  pub(crate) fn parse(text: &str) -> (Self, Vec<String> /* one line per bad entry */);
  pub(crate) fn of(&self, target: &str) -> log::LevelFilter;      // longest matching prefix, else the origin's default
  pub(crate) fn set(&mut self, target: &str, level: log::LevelFilter);
  pub(crate) fn most_verbose(&self) -> log::LevelFilter; }
// mod.rs
pub fn ensure();                                   // installs the logger once; safe from any thread
pub fn set_level(target: &str, level: log::LevelFilter);
pub fn level(target: &str) -> log::LevelFilter;
pub(crate) fn set_frame(frame: u64);               // this thread's
pub(crate) fn installed_by_engine() -> bool;       // false when another logger was set first
// scope.rs
pub(crate) fn current_scope() -> u64;
pub(crate) fn in_scope<R>(scope: u64, f: impl FnOnce() -> R) -> R;   // sets this thread's scope for `f`
pub(crate) fn new_scope() -> u64;                  // never 0, never repeats
// ring.rs  (#[cfg(dev_tools)])
pub(crate) const RING: usize = 10_000;
pub(crate) fn after(seq: u64, scope: Option<u64>, out: &mut Vec<LogRecord>) -> u64;   // appends records with seq > `seq`; returns the newest seq
// capture.rs, re-exported as ryo2d::testing::{capture_log, LogCapture}
pub fn capture_log() -> LogCapture;
impl LogCapture { pub fn records(&self) -> Vec<LogRecord>; pub fn count(&self, level: log::Level) -> usize;
  pub fn warnings(&self) -> Vec<String>; pub fn containing(&self, needle: &str) -> usize;
  pub fn from_target(&self, prefix: &str) -> Vec<LogRecord>; pub fn scope(&self) -> u64; }
// macros.rs, #[macro_export]: trace!, debug!, info!, warn!, error!, warn_once!
```
- **The prelude's macros** call `$crate::logging::ensure()` and then `log::…!(target: concat!("game::", module_path!()), …)`. `warn_once!` keeps one `AtomicBool` per call site. All six are in the prelude.
- **`RYO_LOG`** is read once, when the logger installs: comma-separated entries, each `level` (sets all three defaults) or `target=level`, trimmed, case-insensitive (`off`, `error`, `warn`, `info`, `debug`, `trace`). A bad entry is reported once on stderr and skipped.
- **`log::set_max_level`** follows `Levels::most_verbose()` after every change.
- **Outputs,** each taking a `&LogRecord` after one format of the message:
  - stderr, in a build with tools, as `LEVEL target: message`, written with `eprintln!` so the test harness captures a test thread's lines; a record whose scope has a capture skips stderr;
  - the ring, in a build with tools;
  - the browser console on wasm (`console.error`, `warn`, `info`, `debug` by level);
  - a capture buffer when the record's scope has one;
  - the old `dev_log` buffer (the bridge, removed in Task 12): the record is pushed with the old `LogLevel` and a context from its origin, without printing.
- **No output holds another's lock.** The ring and each capture buffer have their own mutex, held only for the push.
- **A capture** opens `new_scope()` on its thread for its lifetime and restores the earlier scope on drop. It calls `ensure()`.

- [ ] **Step 1: Write the failing tests** (unit tests beside the code; they use `capture_log`, never stderr).
  - Levels:
    - `the_defaults_are_info_for_game_and_engine_and_warn_for_libraries`;
    - `the_longest_prefix_wins` (`ryo2d=warn`, `ryo2d::core::audio=debug`: `ryo2d::core::audio::engine` is debug, `ryo2d::core::ui` is warn);
    - `ryo_log_reads_levels_and_pairs` (`" Debug, wgpu=WARN ,game::menu=trace"`);
    - `a_bad_ryo_log_entry_is_reported_and_skipped` (`"loud,wgpu=7"` gives two problems and leaves the defaults).
  - Origins: `targets_name_their_origin` (`game`, `game::x`, `ryo2d`, `ryo2d::core`, `ryo2d_procgen::noise`, `wgpu_core`, `gamepad` is a library, not the game).
  - Libraries: `a_library_s_warning_is_kept_and_its_info_is_not` (`log::warn!(target: "wgpu_core", ..)` is captured; `log::info!` with that target is not).
  - Another logger: `a_logger_installed_first_is_kept` cannot run in a process that already installed the engine's, so cover the decision in a unit test of the pure function that picks the outcome from `log::set_logger`'s result (`Ok` installs, `Err` reports once and leaves the outputs off).
  - Records and the ring:
    - `a_message_below_its_level_is_never_formatted` (a `Display` whose `fmt` flips a flag, logged at debug under the defaults, leaves the flag clear);
    - `sequence_numbers_never_repeat_and_the_ring_answers_after_n_past_its_size` (log 25,000 records; `after(24_990, ..)` returns the last 10 in order);
    - `a_flood_keeps_the_ring_at_its_size` (100,000 warnings; `after(0, ..)` returns 10,000; Review Focus 5).
  - Macros:
    - `the_prelude_s_macros_log_under_game` (target starts `game::`);
    - `a_line_logged_before_any_engine_is_kept` (Review Focus 2: in a fresh capture, `ryo2d::info!` with no engine built is in `records()`);
    - `warn_once_logs_once_per_call_site` (a loop of 5 over one site gives 1; two sites give 2).
  - Scopes and capture:
    - `capture_collects_its_own_scope_only` (two threads, each with a capture, each logging 100 lines: each sees its own 100);
    - `a_thread_given_a_scope_logs_into_it` (`in_scope(capture.scope(), ..)` on a spawned thread);
    - `the_frame_is_the_logging_thread_s` (`set_frame(7)` then a record has `Some(7)`; a record from another thread has `None`).
  - Live levels: `a_level_can_be_changed_and_changed_back` (restore it at the end of the test).
- [ ] **Step 2: Run them.** `cargo test --lib core::logging`: they fail to compile.
- [ ] **Step 3: Implement.** Write the module notes (`AGENTS.md`): the outputs, the level rules, scopes, capture, and "tests never read the ring".
- [ ] **Step 4: Run them, then `cargo test --workspace` to a log.** All pass (the old log still works; the bridge feeds it).
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add Cargo.toml Cargo.lock src/core/logging src/core/mod.rs src/lib.rs src/core/testing/mod.rs src/core/dev_log.rs
git commit -m "feat: one log on the log crate, with levels per target, scopes and test capture"
```

### Task 4: The log file and the panic hook

**Files:**
- Create: `src/core/logging/file.rs`, `src/core/logging/panic.rs`
- Modify: `src/core/logging/{mod.rs,outputs.rs}`, `src/core/engine/mod.rs` (open the file and install the hook when a windowed native engine starts; flush each frame and at exit)

**Interfaces:**
- Consumes: Task 3.
- Produces:
```rust
// file.rs  (#[cfg(not(target_arch = "wasm32"))])
pub(crate) const CAP: u64 = 8 * 1024 * 1024;
pub(crate) const FLUSH_GAP: std::time::Duration = std::time::Duration::from_millis(50);
pub(crate) fn open(folder: &std::path::Path) -> std::io::Result<std::path::PathBuf>;  // rotates, opens, becomes the file output; returns the path used
pub(crate) fn tick();          // the engine calls it once a frame: flushes when something is unflushed and the gap has passed, or 1 s has
pub(crate) fn flush();         // at exit and from the panic hook
// panic.rs
pub(crate) fn install_hook();  // once per process; chains to the hook that was there
```
- **`open(folder)`** creates `folder/logs/`, renames `latest.log` to `previous.log` (replacing it), and opens `latest.log`. If the rename or the open fails, it opens `latest-<pid>.log`. If that fails, it returns the error; the caller reports it once on stderr and runs without a file.
- **A line** is `HH:MM:SS.mmm LEVEL target: message` (time since the logger started), one per record, in every build.
- **Flushing.** A warning or an error flushes at once unless the last flush was under 50 ms ago; `tick` flushes the rest. The buffer is 8 KiB.
- **The cap.** After 8 MiB the file takes one last line, `log full: no more is written this run`, flushes, and takes no more.
- **Where.** The engine calls `open(&workspace.save_location())` for a windowed native engine only. Headless engines and tests never open a file.
- **The hook** logs `panic at <file>:<line>: <message>` at error level under the target `ryo2d::panic`, calls `flush()`, then the earlier hook. It is installed with the file.

- [ ] **Step 1: Write the failing tests** (they use `tempfile` folders and call `file::open` directly; a test-only `file::close()` detaches the output).
  - `the_previous_run_is_kept_beside_the_new_one`.
  - `a_warning_is_on_disk_at_once_and_an_info_line_after_a_tick`.
  - `a_flood_flushes_at_most_once_a_gap_and_stops_at_the_cap` (Review Focus 5: 100,000 warnings of 100 bytes; the file is at most `CAP` plus one line long and ends with the `log full` line; a counter of flushes in the test build stays under 100,000 / 10).
  - `a_folder_that_cannot_be_written_gives_no_file_and_one_error` (Review Focus 4: a path under a regular file).
  - `a_held_latest_log_falls_back_to_a_pid_name` (Review Focus 4: make `latest.log` a directory so the open fails).
  - `a_panic_is_written_before_the_process_ends` (run `std::panic::catch_unwind` over a panic with the hook installed; the file holds `panic at` and the message).
  - `a_headless_engine_opens_no_log_file` (in `src/core/engine/tests.rs`: after a headless engine runs a frame, its save folder has no `logs`).
- [ ] **Step 2: Run them.** They fail to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them.** They pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/logging src/core/engine/mod.rs src/core/engine/tests.rs
git commit -m "feat: a log file in the save folder, and panics written to it"
```

### Task 5: Engines, frames and worker threads

**Files:**
- Modify:
  - `src/core/engine/mod.rs` (`Engine::new` calls `logging::ensure()` and remembers `logging::current_scope()`; one stderr line when another logger was installed first);
  - `src/core/engine/frame.rs` (`logging::set_frame` at the start of each frame);
  - the spawn sites: `src/core/asset/workers.rs`, `src/core/asset/watch.rs`, `src/core/shaders/compile.rs`, `src/core/saves/{writer.rs,files.rs,service.rs}`, `src/core/tilemap/jobs.rs`, `src/core/capture/writer.rs`.

**Interfaces:**
- Consumes: Task 3's `current_scope` and `in_scope`.
- Produces: every thread the engine spawns runs its body inside `logging::in_scope(scope, ..)`, where `scope` was read on the spawning thread. `src/core/network/` is left alone.

- [ ] **Step 1: Write the failing tests,** one per spawn site, each inside a `capture_log()` and each provoking that site's existing warning through the engine's own API (find the warning each site can log; if a site logs nothing today, log one `log::debug!` line at thread start under its module's target and set that target to debug for the test, restoring it after):
  - `an_asset_worker_s_warning_is_captured` (a queued load of a broken file from a `MemorySource`);
  - `the_file_watcher_logs_into_its_engine_s_scope`;
  - `a_shader_compile_warning_is_captured` (a shader with a WGSL error);
  - `a_save_writer_s_warning_is_captured` (a save into a store that refuses the write);
  - `a_tile_map_job_s_warning_is_captured`;
  - `the_capture_writer_logs_into_its_engine_s_scope`;
  - `two_engines_stamp_their_own_frames` (two headless engines on two threads, stepped 3 and 5 frames, each inside its own capture: each capture's last record carries its own engine's frame).
- [ ] **Step 2: Run them.** They fail: the workers' records are in scope 0.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them, then `cargo test --workspace` to a log.** All pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/engine src/core/asset src/core/shaders/compile.rs src/core/saves src/core/tilemap/jobs.rs src/core/capture/writer.rs
git commit -m "feat: engine threads log into their engine's scope, and records carry their frame"
```

### Task 6: Move every log call to the new macros

**Files:**
- Modify: every file under `src/`, `crates/` and `examples/` that calls `engine_*!`, `game_*!`, `dev_log!` or `dev_log::log` (about 220 sites), and every `println!`/`eprintln!` in engine library code outside `src/cli/`, `src/core/network/` and `src/core/logging/` (about 70 sites).

**Interfaces:**
- Consumes: Task 3's macros.
- Produces: no call site of the old macros is left. The old macros and `dev_log`'s buffer stay until Task 12 (the logger window and some tests still read the buffer, fed by the bridge).

**Rules:**
- `engine_X!(…)` becomes `log::X!(…)`. A `module: "Name",` argument is dropped: the target is the module path.
- `game_X!(…)` becomes `X!(…)` from the prelude (`ryo2d::X!` where the prelude is not imported).
- `crates/ryo2d-procgen`'s direct `dev_log::log` becomes `log::warn!` (add `log = "0.4"` to that crate).
- Every message keeps its exact text, so tests that match on it still pass.
- An engine `println!`/`eprintln!` that reports a problem becomes `log::warn!` or `log::error!`; one that reports progress (`[capture]`, `[headless]`, `[input]` lines, the dev windows' "opened" lines) becomes `log::info!`.
- The startup banner (`src/core/engine/mod.rs` ~1028-1048) stays a `println!`, but under `#[cfg(dev_tools)]`, and it lists a key only when that key is on.
- Tests that filter the old buffer by `module == "Name"` (for example `particles::testing::particle_warnings`, which matches `"Particles"`) switch to the bridge's module value, which is the last `ryo2d::core::<name>` segment of the target with its first letter upper-cased. Write the bridge that way in this task if Task 3's differs, so those filters keep working until Task 7.

- [ ] **Step 1: Move the call sites, module by module.** After each top-level module, run `cargo check --workspace`.
- [ ] **Step 2: Search for leftovers.** `rg "engine_(trace|debug|info|warn|error)!|game_(trace|debug|info|warn|error)!|dev_log!\(|dev_log::log\(" src crates examples tests benches` finds only the macro definitions in `src/core/dev_log.rs` and Task 1's bench. `rg "println!|eprintln!" src/core --glob '!**/network/**' --glob '!**/logging/**'` finds only the banner and test code.
- [ ] **Step 3: Run `cargo test --workspace` to a log.** All pass, with nothing edited during the run.
- [ ] **Step 4: Run the checks, then commit.**
```bash
git add -u src crates examples benches
git commit -m "refactor: the engine, the crates and the examples log through the log crate"
```
(`git add -u` with paths stages only tracked files under them. Check `git status --short` first: nothing under `data/`, `target/` or `dist/` may be staged.)

### Task 7: Tests read their own log

**Files:**
- Modify: every test and test helper that calls `dev_log::get_entries`, `get_entries_since` or `entry_count` (94 sites): the helpers `src/core/ui/test_util.rs` (`warnings_containing`), `src/core/particles/testing.rs` (`particle_warnings`, `log_lock`), `src/core/engine/tests.rs` (`settings_warnings`), `src/core/testing/shader_gpu.rs`, and tests under `src/core/{tilemap,ui,text,audio,shaders,asset,physics,gameplay,engine,config,scene,saves,render,graphics,navigation}`, `crates/ryo2d-procgen`, `tests/e2e/`, and the examples' tests (breakout, tilemap, character, leap-king, tile-grids, saves, particles, audio).

**Interfaces:**
- Consumes: `ryo2d::testing::capture_log` and `LogCapture`.
- Produces: no test reads the old buffer. `particles::testing::log_lock` and every use of it are gone.

**Rules:**
- A test that asserts on the log opens `let log = capture_log();` before the code that logs (before the engine or world is built, so the engine's threads join its scope), and asserts on `log`.
- A helper that counted in the shared buffer takes `&LogCapture`.
- Counts stay exact (`assert_eq!(…, 1)`), and the unique-needle text stays.
- Tests that filtered by the old `module` name filter by target prefix (`log.from_target("ryo2d::core::particles")`).
- A test that needs a level other than the default sets it with `logging::set_level` and restores it before it ends.

- [ ] **Step 1: Move the three shared helpers and their callers, then the rest, module by module.** Run each module's tests after its move.
- [ ] **Step 2: Search for leftovers.** `rg "dev_log::(get_entries|get_entries_since|entry_count|clear)|log_lock" src crates examples tests` finds only `src/core/dev_log.rs` and `src/core/windows/logger_window.rs`.
- [ ] **Step 3: Run `cargo test --workspace` to a log, twice.** Both runs pass.
- [ ] **Step 4: Run the checks, then commit.**
```bash
git add -u src crates examples tests
git commit -m "test: every test that reads the log captures its own"
```

### Task 8: Commands and the registry

**Files:**
- Create: `src/core/console/command.rs` (always compiled), `src/core/console/registry.rs` (`#[cfg(dev_tools)]`)
- Modify: `src/core/console/mod.rs` (module declarations only; the old `commands.rs` stays in use until Task 9), `src/core/scene/mod.rs` (`pub trait Scene: Any + 'static`)

**Interfaces:**
- Produces:
```rust
// command.rs
#[derive(Debug, Clone)] pub struct CommandSpec { pub name: String, pub description: String, pub usage: Option<String>, pub help: Option<String> }
impl CommandSpec { pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self;
  pub fn usage(self, usage: impl Into<String>) -> Self; pub fn help(self, help: impl Into<String>) -> Self; }
impl From<(&str, &str)> for CommandSpec;
pub struct Command<'a> { /* name, args, usage, printed lines, requested action */ }
impl Command<'_> {
  pub fn name(&self) -> &str; pub fn args(&self) -> &[String];
  pub fn arg<T: std::str::FromStr>(&self, index: usize) -> Result<T, String>;       // "argument 2 (\"abc\") is not a number; usage: game:spawn <kind> [count]"
  pub fn arg_or<T: std::str::FromStr>(&self, index: usize, default: T) -> Result<T, String>;   // the default when the argument is absent
  pub fn rest(&self, from: usize) -> String;                                         // the arguments from `from` on, joined by spaces
  pub fn print(&mut self, text: impl Into<String>); pub fn print_colored(&mut self, text: impl Into<String>, color: Color);
  pub fn act(&mut self, action: SceneAction);                                        // applied when the handler returns
  pub fn quit(&mut self); }
pub(crate) fn split(line: &str) -> Vec<String>;      // words; double quotes keep a phrase; a lone quote runs to the end of the line
pub type SceneHandler<S> = dyn Fn(&mut S, &mut SceneContext<'_>, &mut Command<'_>) -> Result<(), String> + Send + Sync;
pub type WorldHandler = dyn Fn(&mut World, &mut Command<'_>) -> Result<(), String> + Send + Sync;
// registry.rs
pub(crate) enum Handler { Builtin(Arc<BuiltinHandler>), World(Arc<WorldHandler>), Scene { scope: u64, call: Arc<ErasedSceneHandler> } }
pub(crate) type BuiltinHandler = dyn Fn(&mut World, &mut EngineHooks<'_>, &mut Command<'_>) -> Result<(), String> + Send + Sync;
pub(crate) type ErasedSceneHandler = dyn Fn(&mut dyn Scene, &mut SceneContext<'_>, &mut Command<'_>) -> Result<(), String> + Send + Sync;
pub(crate) fn erase<S: Scene>(handler: impl Fn(&mut S, &mut SceneContext<'_>, &mut Command<'_>) -> Result<(), String> + Send + Sync + 'static) -> Arc<ErasedSceneHandler>;
#[derive(Resource, Default)] pub(crate) struct ConsoleCommands { /* per name: a stack of (spec, handler), the newest last */ }
impl ConsoleCommands {
  pub(crate) fn add(&mut self, spec: CommandSpec, handler: Handler);   // name lower-cased; a scene re-adding its own name replaces its entry
  pub(crate) fn remove(&mut self, name: &str) -> bool;                 // every entry of the name
  pub(crate) fn clear_scope(&mut self, scope: u64);
  pub(crate) fn find(&self, name: &str) -> Option<(CommandSpec, Handler)>;   // the newest entry, cloned out
  pub(crate) fn list(&self) -> Vec<CommandSpec>;                       // the answering entry of each name, sorted by name
  pub(crate) fn completions(&self, prefix: &str) -> Vec<String>; }     // sorted names starting with `prefix`
pub(crate) struct EngineHooks<'a> { pub(crate) debug: &'a mut DebugConfig, pub(crate) overlay: &'a mut OverlayMode,
  pub(crate) capture: &'a mut Option<CaptureTarget>, pub(crate) quit: &'a mut bool }
```
- `erase` downcasts `&mut dyn Scene` to `&mut S` through `Any`; when the scene is another type it returns `Err("<name> was registered for another scene type")`.
- The overlay mode type is whatever F1 cycles today in `src/core/engine/frame.rs` (`next_overlay`); name it `OverlayMode` and make it `pub(crate)` if it is not.

- [ ] **Step 1: Write the failing tests.**
  - `command.rs`:
    - `words_split_on_spaces_and_quotes_keep_phrases` (`say "hello there" now` gives 3 words; `say "open` gives 2);
    - `a_typed_argument_parses_or_names_itself` (the error holds the index, the text and the usage);
    - `a_missing_argument_takes_its_default`;
    - `rest_joins_what_is_left`.
  - `registry.rs`:
    - `names_are_lower_cased_so_any_spelling_runs`;
    - `two_scopes_adding_one_name_stack_and_the_earlier_returns` (Review Focus 1);
    - `a_scope_re_adding_its_own_name_replaces_it`;
    - `clearing_a_scope_takes_only_its_own`;
    - `the_list_shows_each_name_once_sorted`;
    - `completions_are_the_names_with_the_prefix`;
    - `an_erased_handler_refuses_another_scene_type`.
- [ ] **Step 2: Run them.** `cargo test --lib core::console::command core::console::registry`: they fail to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them.** They pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/console src/core/scene/mod.rs
git commit -m "feat: console commands with typed arguments and a registry that stacks by scene"
```

### Task 9: The console runs commands in the game

**Files:**
- Create: `src/core/console/{builtins.rs (rewritten),dispatch.rs,scope.rs}`, `src/core/console/AGENTS.md`
- Delete: `src/core/console/commands.rs`
- Modify:
  - `src/core/console/mod.rs` (`ConsoleState` becomes the world resource `Console`, `pub(crate)`);
  - `src/core/scene/{context.rs,manager.rs}` (`ConsoleScope`, running a scene's handler through its slot, `clear_scope`);
  - `src/core/engine/{mod.rs,frame.rs,window.rs}` (`console_tick`, the console as a resource everywhere it is used, the builder's calls);
  - `src/core/testing/headless.rs` (`console_run`);
  - `src/lib.rs` (prelude: `Command`, `CommandSpec`, `ConsoleScope`; the old names go);
  - the users: `examples/demos/console/main.rs`, `examples/games/bullet-hell/src/{shared.rs,scenes/menu.rs,scenes/game/mod.rs}`, `examples/games/leap-king/src/{main.rs,scenes/game.rs}`, `tests/integration/console.rs`, `src/core/engine/tests.rs`, and every fenced Rust block in `.claude/skills/` that uses the old console API (code only; no version bumps).

**Interfaces:**
- Consumes: Task 8.
- Produces:
```rust
// scope.rs: what ctx.console() returns, in every build
impl ConsoleScope<'_> {
  pub fn command<S: Scene>(&mut self, spec: impl Into<CommandSpec>,
    handler: impl Fn(&mut S, &mut SceneContext<'_>, &mut Command<'_>) -> Result<(), String> + Send + Sync + 'static);
  pub fn remove(&mut self, name: &str) -> bool;                  // any command, a built-in included
  pub fn print(&mut self, text: impl Into<String>);
  pub fn print_colored(&mut self, text: impl Into<String>, color: Color); }
// EngineBuilder
pub fn console_command(self, spec: impl Into<CommandSpec>,
  handler: impl Fn(&mut World, &mut Command<'_>) -> Result<(), String> + Send + Sync + 'static) -> Self;
pub fn remove_console_command(self, name: impl Into<String>) -> Self;
// HeadlessEngine  (#[doc(hidden)], #[cfg(dev_tools)])
pub fn console_run(&mut self, line: &str) -> Vec<String>;        // runs the line as if typed; the lines it printed
```
- **Running a line** (`dispatch.rs`, called from `console_tick` on a submitted line and from `console_run`):
  1. Split it; the first word lower-cased is the name. An empty line does nothing. An unknown name prints `Unknown command: <name>. Type 'help'.` in red.
  2. `ConsoleCommands::find` clones the spec and handler out, so the registry is not borrowed while the handler runs (Review Focus 3).
  3. A built-in runs with the world and `EngineHooks` built from the engine's own fields. A builder command runs with the world. A scene command runs through the scene host: the slot with that scope id is called as `update` is (inside its scope), with the handler given `&mut dyn Scene`, the `SceneContext` and the command. A scope with no slot (the scene left) prints an error and removes the entry.
  4. The command's printed lines go to the console. `Err(text)` prints in red. A requested action goes through the manager's `handle_action`, as `update`'s does; `quit` asks the engine to quit.
- **Built-ins** (the same set as today, less the two `system:ip:*`, plus five):
  - `help [name]`: with no argument, every command from `ConsoleCommands::list`, grouped by the text before the first `:` (commands with no `:` first), each with its description; with a name, its description, usage and long help. Unknown names say so.
  - `clear`, `quit`, `exit`.
  - `debug:fps` sets `Settings::show_fps`. `debug:stats` and `debug:timing` set `hooks.overlay` to the mode F1 would reach, so F1 carries on from it.
  - `debug:colliders`, `debug:velocities`, `debug:origins`, `debug:ui`, `debug:ui-rebuilds`, `ui:cache`, `ui:fragments`, `ui:retain`, `debug:capture [path]`: as today, through `hooks`.
  - `system:timescale [n]` reads or sets `Time`'s game speed; it refuses NaN, a negative number and anything above 100 (the clock's own range).
  - `system:version`: as today.
  - `log:level [target] [level]`: with none, prints the three defaults and each override; with a target, prints its level; with both, sets it.
  - `log:show [level|off]`: the level the console mirrors (Task 10 does the mirroring; this task stores the setting on `Console`, default `warn`).
  - `system:scenes`: the scene stack, top first, with each scene's type name and whether it updates.
  - `system:entities`: the count of game entities (as the F1 stats count them).
  - `system:assets`: the registry's stats (`AssetServer` stats, as `ctx.assets().stats()` gives).
- **The categories `debug`, `system`, `game`** as bare commands go; `help` covers them.
- **Module notes** (`console/AGENTS.md`): the three handler kinds, how a line runs, the stack per name, what a build without tools keeps.

- [ ] **Step 1: Write the failing tests,** through `HeadlessEngine::console_run` unless noted (in `tests/integration/console.rs`, rewritten, and `src/core/engine/tests.rs`).
  - Scene commands:
    - `a_scene_s_command_calls_its_scene_and_changes_the_world` (the handler bumps a field on the scene and spawns an entity);
    - `a_scene_s_command_can_switch_scene` (`cmd.act(SceneAction::Switch(..))`: the next frame runs the new scene, and the old one's command is unknown);
    - `the_upper_scene_answers_and_the_lower_one_returns` (Review Focus 1: two scenes register `game:who`; a pushed pause scene over them changes nothing; after a pop the lower answers; each printed its own type);
    - `a_handler_can_change_the_console_and_pop_its_scene` (Review Focus 3: it registers a command, removes another, and pops; no panic; its own commands are gone next frame);
    - `a_command_s_sound_and_particles_belong_to_its_scene` (a particle one-shot played from the handler is held when that scene is, using the particle store's hold test hooks).
  - Builder commands: `a_builder_command_changes_the_world_and_outlives_scenes`; `removing_a_built_in_takes_it_from_help_too`.
  - Results: `a_handler_s_error_prints_in_red`; `a_bad_argument_prints_the_usage`; `quit_asks_the_engine_to_quit`; `an_unknown_command_says_so`; `a_name_typed_with_capitals_runs`.
  - Built-ins:
    - `help_lists_every_command_and_only_those` (every name in `ConsoleCommands::list` appears, and a removed one does not);
    - `debug_fps_and_f1_agree` (after `debug:fps`, `Settings::show_fps` is true and one F1 press moves to the next mode, not the first);
    - `debug_stats_and_timing_set_the_overlay_mode`;
    - `timescale_sets_the_clock_and_refuses_nan`;
    - `log_level_shows_and_sets` (restore the level after);
    - `scenes_entities_and_assets_report`;
    - `no_command_names_an_ip` (`help` has no `ip`).
  - A unit test in `src/core/console/mod.rs`: `the_console_names_no_network_code` (the module's source files, read with `include_str!`, do not contain `std::net`).
- [ ] **Step 2: Run them.** They fail to compile.
- [ ] **Step 3: Implement, and move the users over.**
  - The console demo: `game:hello` and `game:paint` become scene commands; `game:paint` sets fields on its page in place of the `PAINT_R/G/B` atomics.
  - Bullet-hell: `game:reset` is registered by the game scene and does `cmd.act(SceneAction::Switch(Box::new(GameScene::new())))`; `shared::request_reset` and its atomic go, and so does the menu's declaration.
  - Leap-king: `game:respawn` is registered by the game scene and calls its `execute_respawn`; `CONSOLE_RESPAWN_REQUESTED` and the builder's declaration go.
- [ ] **Step 4: Run them, the examples' tests (`cargo test -p console-demo -p bullet-hell -p leap-king -p skill-examples`), then `cargo test --workspace` to a log.** All pass.
- [ ] **Step 5: Search for leftovers.** `rg "CommandContext|CommandResult|ConsoleCommandBuilder|register_handler|with_console_commands|CommandRegistry" src examples tests .claude` finds nothing.
- [ ] **Step 6: Run the checks, then commit.**
```bash
git add src/core/console src/core/scene src/core/engine src/core/testing/headless.rs src/lib.rs examples/demos/console examples/games/bullet-hell examples/games/leap-king tests/integration/console.rs .claude/skills
git commit -m "feat: console commands run in the game: a scene's gets its scene, and built-ins use the engine's own state"
```
(Stage the deleted `commands.rs` with `git rm`.)

### Task 10: The console's window

**Files:**
- Modify: `src/core/console/mod.rs` (split the drawing into `src/core/console/view.rs` if `mod.rs` stays over 1,500 lines), `src/core/engine/frame.rs` (pointer units, mouse buttons)

**Interfaces:**
- Consumes: Tasks 3 and 9.
- Produces: no new public names.

**Behaviour:**
- **Wrapping.** Each line keeps its wrapped rows for the width it was wrapped at. A frame wraps only lines that are new or whose width changed (a resize, or the scrollbar appearing). The total row count is kept, so scrolling no longer reads last frame's count.
- **Completion.** Tab on a partial first word completes it to the longest common prefix of `ConsoleCommands::completions`; a second Tab with several matches prints them on one line. Tab after the first word does nothing.
- **The log in the console.** Each frame the console appends the ring's records after the last sequence it showed, in its engine's scope, at or above its `log:show` level: warnings in yellow and errors in red, as `target: message`. `log:show off` stops it. The mirror never shows a record the console itself caused to be logged while printing (no loop).
- **The mouse.** Every mouse button pressed over the console's window is consumed after the UI pass, not only the left. The console's pointer is converted to UI units before its hit tests, so they match its drawing under an explicit `ui_scale` (the conversion the UI pass uses). The clamped position is written back, so hit tests and drawing agree after the game window shrinks.
- **Typing.** The backquote and tilde can be typed once the console is open; only the key press that toggles it is swallowed.

- [ ] **Step 1: Write the failing tests.**
  - `a_held_open_console_wraps_each_line_once` (a counter of wrap calls in the test build: 1,000 lines printed, 60 frames drawn, the counter is 1,000, or 2,000 when the scrollbar appeared);
  - `a_resize_wraps_again_and_new_lines_wrap_alone`;
  - `tab_completes_a_command_name_and_lists_several`;
  - `warnings_and_errors_appear_in_the_console_and_info_does_not`;
  - `log_show_off_stops_the_mirror`;
  - `two_engines_mirror_only_their_own_warnings` (two headless engines, each in its own capture);
  - `every_mouse_button_over_the_console_stays_out_of_the_game`;
  - `hit_tests_match_the_drawing_under_a_ui_scale`;
  - `a_backquote_can_be_typed_into_a_command`.
- [ ] **Step 2: Run them.** They fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them, and the console's existing tests.** They pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/console src/core/engine/frame.rs
git commit -m "feat: the console wraps each line once, completes names, shows warnings and takes every click"
```

### Task 11: The profiler window closes cleanly

**Files:**
- Modify: `src/core/windows/profiler_window.rs`, `src/core/profiler.rs`, `src/core/engine/{window.rs,frame.rs,mod.rs}`, `src/cli/commands/dev.rs`, the scope names in `src/core/engine/extract.rs`, `src/core/graphics/{renderer.rs,atlas.rs}`

**Interfaces:**
- Produces: no new public names.

**Behaviour:**
- `ProfilerWindow` implements `Drop`: it removes its sink (`puffin::GlobalProfiler::lock().remove_sink(id)`).
- When the window closes and the project has not set `[debug] enable_profiler`, scopes go off (`puffin::set_scopes_on(false)`); opening turns them on.
- Closing either dev window with its X button clears that window's focus flag, as the F-key path does (`src/core/engine/window.rs` ~726 and ~740). The logger window's flag is fixed here too.
- The pending-open branch for the profiler is skipped on wasm.
- Scope names: `render:particles` and `render:submit` become `cpu:render:particles` and `cpu:render:submit`; `atlas:spread texture` and `atlas:spread first band` become `cpu:atlas:spread texture` and `cpu:atlas:spread first band`.
- `ryo2d dev -p` passes `--features ryo2d/profiler` (no game feature needed), its missing-feature help goes, and the `--help` text no longer mentions an HTTP server.
- The games' and demos' own `profiler = ["ryo2d/profiler"]` features are removed (`examples/games/{bullet-hell,flappy,leap-king,nigh_t,breakout}`, `examples/demos/profile-and-log`), and `dev.sh`/`dev.ps1` follow.
- `profiler::init`'s log line says what is true ("profiling scopes are on").

- [ ] **Step 1: Write the failing tests** (under `#[cfg(feature = "profiler")]`, in `profiler_window.rs` and `engine/tests.rs`; the window's sink logic is split into a small type that needs no window, so it can be tested headless):
  - `opening_and_closing_five_times_leaves_no_sink_and_scopes_off` (count sinks through a test-only counter the sink type keeps);
  - `scopes_stay_on_when_the_project_asked_for_them`;
  - `closing_with_the_x_button_clears_the_focus_flag` (drive the engine's window-event handler with `CloseRequested` for a fake dev window id through a crate-internal helper).
- [ ] **Step 2: Run them.** `cargo test --lib --features profiler profiler`: they fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them, and `cargo check --workspace --features ryo2d/profiler`.** They pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/windows/profiler_window.rs src/core/profiler.rs src/core/engine src/cli/commands/dev.rs src/core/graphics examples dev.sh dev.ps1
git commit -m "fix: the profiler window removes its sink and turns scopes off, and games need no profiler feature"
```

### Task 12: The logger window follows the log, and the old log goes

**Files:**
- Modify: `src/core/windows/logger_window.rs`, `src/core/windows/{mod.rs,AGENTS.md}`, `src/core/logging/{mod.rs,outputs.rs}` (the bridge goes), `src/lib.rs`, `src/core/mod.rs`, `src/cli/commands/check_ui.rs`, `benches/dev_tools.rs` (Task 1's two log benches move to `log::debug!` and `log::warn!`, with the same names and counts)
- Delete: `src/core/dev_log.rs`

**Interfaces:**
- Consumes: Task 3's ring.
- Produces: `logging::set_stderr(on: bool)` (`pub(crate)`), which `check_ui` uses where it called `set_stdout_enabled`.

**Behaviour:**
- The window keeps the last sequence number it read and asks the ring for what came after, in its engine's scope. It never reads by index or length.
- A row shows the time, the frame (blank when none), the level letter, the origin letter (`E`, `G`, `L`) and `target: message`.
- Buttons: the five levels, `ENG`, `GAME`, `LIB` (off by default), `CLR` (clears the window's own list, not the ring) and `BTM`.
- The wheel scrolls three rows a notch. A trackpad's pixel delta is divided by the window's scale factor, then by the row pitch.
- The dead parts go: `search_text`, `update_filtered_entries`, the unreachable trim, the `--logger` mention in `windows/mod.rs`.

- [ ] **Step 1: Write the failing tests.** Split the window's reading and filtering into a type that needs no window (`LogFeed`), and test that:
  - `the_feed_shows_record_10_001_and_after` (log 10,500 records; the feed's newest row is the last one);
  - `the_feed_reads_only_its_scope`;
  - `library_rows_show_only_with_lib_on`;
  - `clr_empties_the_list_and_the_feed_carries_on`;
  - `a_trackpad_delta_scrolls_by_logical_pixels` (a 28-physical-pixel delta at scale 2 scrolls one 14-pixel row).
- [ ] **Step 2: Run them.** They fail.
- [ ] **Step 3: Implement, then delete `dev_log.rs` and the bridge.**
- [ ] **Step 4: Search for leftovers.** `rg "dev_log" src crates examples tests benches .claude/skills` finds nothing.
- [ ] **Step 5: Run the tests and `cargo test --workspace` to a log.** All pass.
- [ ] **Step 6: Run the checks, then commit.**
```bash
git add src/core/windows src/core/logging src/lib.rs src/core/mod.rs src/cli/commands/check_ui.rs benches/dev_tools.rs
git commit -m "fix: the logger window follows the log by sequence for a whole session, and the old log is removed"
```
(Stage the deleted `dev_log.rs` with `git rm`.)

### Task 13: A build without tools

**Files:**
- Modify:
  - `src/core/console/{mod.rs,scope.rs}` and `src/core/mod.rs` (everything but `command.rs` and the `ConsoleScope` front is `#[cfg(dev_tools)]`; without tools `ConsoleScope`'s methods do nothing and `remove` returns false);
  - `src/core/windows/mod.rs` (the whole module is `#[cfg(all(dev_tools, not(target_arch = "wasm32")))]`);
  - `src/core/engine/{mod.rs,frame.rs,window.rs,extract.rs}` (the console resource and tick, the dev window fields, creation, event routing and redraws, the debug keys, the banner);
  - `src/core/diagnostics/flags.rs`, `src/core/config/{project_table.rs,project.rs,resolve.rs,workspace.rs}`;
  - `src/cli/commands/{build.rs,init.rs}`;
  - `tests/no_tools.rs`;
  - the examples' `Ryo.toml` files that set `[system] console`.

**Interfaces:**
- Consumes: every earlier task.
- Produces: `ryo2d build --dev-tools`; the Ryo.toml key `[debug] console`.

**Behaviour:**
- **Without tools:** no `Console` or `ConsoleCommands` resource, no dispatch, no built-ins, no dev windows or their fields, no log ring, no banner. `handle_debug_keys` handles F5 only (under `disable_debug_keys`, as today) and consumes no other key. `EngineBuilder::console_command` and `remove_console_command` drop their arguments. The resolved debug flags that switch tools or overlays on are forced off whatever the project says: `dev_console`, `dev_window_keys`, `logger_window`, `enable_profiler`, `show_colliders`, `show_velocities`, `show_origins`, `show_frame_timing`, and the UI debug overlays.
- **With tools:** `dev_console` and `dev_window_keys` default to true. Ryo.toml's `[debug] console`, `dev_window_keys` and the rest can turn each off.
- **Ryo.toml.** The console's key is `[debug] console` (embed key unchanged). `[system] console` is no longer a key, so the existing unknown-key warning reports it and suggests `debug.console`. The examples that set it move it; `ryo2d init`'s template drops its `console = false` line.
- **`ryo2d build`.** It embeds only the `[debug]` values the project set (no forcing in `ProjectLayer::for_release`). `release_tool_warnings` goes. `--dev-tools` adds `--features ryo2d/dev-tools` and prints `this build includes developer tools: do not ship it`.
- **F1 without tools** does nothing. The FPS counter still draws when `Settings::show_fps` is set.

- [ ] **Step 1: Write the failing tests.**
  - In `tests/no_tools.rs` (run with `cargo test --release --test no_tools`):
    - `console_calls_compile_and_do_nothing` (a scene registers a command and prints; a builder command is added; the engine steps);
    - `the_debug_keys_reach_the_game` (F1 to F4, F9, F10, F12 and the backquote are still pressed for the scene after the engine's key handling);
    - `f5_still_toggles_borderless`;
    - `a_project_cannot_switch_tools_on` (a resolve with every `[debug]` tool flag true gives them all false);
    - `the_fps_counter_still_follows_its_setting`.
  - In the development test run:
    - `the_console_and_dev_keys_default_on_with_tools` (`src/core/config` tests);
    - `debug_console_is_the_key_and_system_console_warns` (the warning names `debug.console`);
    - `a_release_build_embeds_only_what_the_project_set` and `dev_tools_adds_the_feature_and_one_warning` (`src/cli/commands/build.rs` tests, on the command it would run).
- [ ] **Step 2: Run them.** They fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them.** `cargo test --release --test no_tools` to a log: all pass. `cargo test --workspace` to a log: all pass. `cargo check -p ryo2d --lib --release` and `cargo check -p console-demo --release` pass with no warnings (no dead code left behind by the gating).
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/console src/core/windows/mod.rs src/core/mod.rs src/core/engine src/core/diagnostics/flags.rs src/core/config src/cli/commands tests/no_tools.rs examples
git commit -m "feat: a shipped game has no dev tools, and ryo2d build --dev-tools keeps them"
```

### Task 14: The demos

**Files:**
- Modify: `examples/demos/console/` (restructure as `examples/demos/saves` is laid out: `src/lib.rs`, `src/main.rs`, `tests/pages.rs`), `examples/demos/profile-and-log/main.rs`

**Behaviour:** pages follow the demo-kit standard (`examples/demos/AGENTS.md`).
- **Console demo:**
  - **Commands:** `game:spawn <kind> [count]` and `game:clear` add and remove entities in the world, with typed arguments and an error for a bad one; `game:say "<phrase>"` shows a quoted phrase on screen.
  - **Scenes:** a page with a scene-only command (`game:paint <r> <g> <b>`) that sets fields on the page, and a line saying it is gone on the other pages.
  - **Log:** buttons that log at each level, from the game and from a library target, with the console mirroring warnings and errors; a line telling the reader to try `log:level game debug` and `log:show info`.
- **Profile-and-log demo:** logs through the new macros at every level, and once through `log::warn!(target: "some_library", …)`.

- [ ] **Step 1: Write `tests/pages.rs`.** Each page builds and steps 120 headless frames with no warning in its capture; `game:spawn slime 3` through `console_run` leaves 3 more entities; `game:spawn slime many` prints the usage; `game:paint` is unknown on the other pages. It fails to compile.
- [ ] **Step 2: Build the pages.**
- [ ] **Step 3: Run the tests.** `cargo test -p console-demo` passes. `./dev.sh demos:tour console profile-and-log` (to a log) captures every page.
- [ ] **Step 4: Run the checks, then commit.**
```bash
git add examples/demos/console examples/demos/profile-and-log
git commit -m "feat: the console demo shows commands that act on the game, and the log in the console"
```

### Task 15: A long session, and nothing left behind

**Files:**
- Modify: whatever the checks below find.
- Test: `tests/e2e/dev_tools.rs` (registered in `tests/e2e.rs`)

**Behaviour:** a sweep, with tests, for what the earlier tasks could leave behind.

- [ ] **Step 1: Write the tests.**
  - `a_long_session_keeps_every_tool_working`: a headless engine with tools runs 2,000 frames while a system logs 30,000 warnings; afterwards the console's mirror shows the newest warning, a `LogFeed` shows the newest record, the ring holds 10,000, and the console holds 1,000 lines.
  - `tools_closed_cost_nothing_they_can_avoid`: with the console closed and no dev window, 100 frames make no allocation in `ryo2d::core::console` or `ryo2d::core::windows` code (by the method of `tests/particles_alloc.rs`; put this one in a new `tests/dev_tools_alloc.rs`).
  - `an_open_idle_console_allocates_nothing_of_its_own`: the same, with the console open on 1,000 lines and nothing new printed.
- [ ] **Step 2: Run them.** Fix what they find in the console or logging code.
- [ ] **Step 3: Sweep for leftovers.**
  - `rg "debug_assertions" src/core/{console,windows,logging,diagnostics}` and `src/core/engine`: any use that means "dev tools" becomes `dev_tools`.
  - `rg "cfg\(dev_tools\)" src | wc -l` and read each `not(dev_tools)` branch once: none logs, prints or keeps a field only the tools read.
  - `cargo doc --no-deps -p ryo2d` gives no warning beyond the known `padded.rs` links.
- [ ] **Step 4: Run `cargo test --workspace` and `cargo test --release --test no_tools` to logs.** All pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add tests/e2e.rs tests/e2e/dev_tools.rs tests/dev_tools_alloc.rs src
git commit -m "test: the dev tools hold through a long session and cost nothing while closed"
```
(Stage `src` files by name if any changed.)

### Task 16: Measure and guard

**Files:**
- Modify: `benches/dev_tools.rs`, `docs/backlog/dev-tools.md`, `src/core/testing/headless.rs` (a `#[doc(hidden)]` timer for the console's and dev windows' own per-frame time, if needed)

**The bench** (`cargo bench` builds without tools, so the tools' benches run with `--features dev-tools`; say so at the top of the file):
- `log_call_quiet` (`log::debug!` at the default levels): target ≤ 5 ns.
- `log_call_warn` (`log::warn!` with stderr to the null device, the ring on, a file in a temp folder): recorded, and guarded at no slower than Task 1's `log_call_warn` median.
- `console_open_idle`: guarded at under a quarter of Task 1's median.
- `tools_closed` (with `--features dev-tools`: the console's and dev windows' own time a frame, closed): target ≤ 1 µs.
- Recorded with no target: `cargo build --release -p console-demo` binary size, and the same with `--features ryo2d/dev-tools`; whether `strings` finds `system:timescale` in each (it must not in the first and must in the second: assert this in the task's report, and record it).

- [ ] **Step 1: Add the guards** (`guards::miss`, `guards::verdict`, with Task 1's medians as constants and a comment saying where they came from).
- [ ] **Step 2: Run the bench three times each way** (`cargo bench --bench dev_tools` and `cargo bench --bench dev_tools --features dev-tools`), recording `uptime` before each, to logs.
- [ ] **Step 3: Build the two release binaries and check the marker strings.**
- [ ] **Step 4: Record it.** Add "### After" to `docs/backlog/dev-tools.md` with the Before and After table, the binary sizes and the marker check, and "## Limits":
  - the dev windows are native only, and the profiler is not on the web;
  - `tracing` output (winit's) is not read;
  - a game calling `log::info!` directly counts as a library;
  - a log line before the first prelude macro or engine is lost if it came from a library;
  - the per-system "warn once" sets are not shared;
  - the file's flush gap means up to 50 ms of warnings can be lost in a hard crash that skips the panic hook;
  - a scene's command cannot be typed while its scene is off the stack.
  - **On a miss,** report DONE_WITH_CONCERNS with the figures. Don't loosen a target.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add benches/dev_tools.rs docs/backlog/dev-tools.md src/core/testing/headless.rs
git commit -m "test: the log and the console are guarded against their targets and recorded"
```

### Task 17: Docs

**Files:**
- Modify skills, each bumped one minor version:
  - `.claude/skills/ryo-console/SKILL.md` (rewritten: registering a scene command and a builder command, `Command`'s arguments and printing, scene actions, the built-ins, `help` and Tab, the log in the console, what a shipped game has);
  - `ryo-profiler` (scopes are recorded only with the profiler feature and while they are on; every engine scope by its new name; `ryo2d dev -p` needs no game feature; the log section moves to the new macros);
  - `ryo-cli` (`ryo2d build --dev-tools`, the dropped warnings, `RYO_LOG`);
  - `ryo-api` (the console and logging sections, `DEV_TOOLS`, `capture_log`);
  - `ryo-game-dev` (logging with `info!` and the rest, `warn_once!`, the log file, console commands in a scene);
  - `ryo-engine-dev` (the `dev_tools` condition, `log::` in engine code, scopes at spawn sites, the no-tools test target);
  - `ryo-config-save` (`[debug] console`, the dev flags' defaults).
- Modify module notes: `src/core/logging/AGENTS.md`, `src/core/console/AGENTS.md`, `src/core/windows/AGENTS.md`, `src/core/AGENTS.md`, `src/core/config/AGENTS.md`, `src/core/testing/AGENTS.md` (capture), `src/cli/AGENTS.md`.
- Modify other docs: `docs/architecture/overview.md` (every line naming the console, the log or the dev windows), `README.md`.
- Update `docs/backlog/follow-ups.md`: remove the lines this branch fixed (the console's `debug:fps` and `Settings`; the console re-wrapping every frame; the console's pointer under `ui_scale`; the logger window's trackpad speed; the warning-count flake notes that capture retires). Leave the rest.

- [ ] **Step 1: Rewrite and update the docs above.** Check every API name, signature and default against the code. Every fenced Rust example compiles.
- [ ] **Step 2: Run the tests.** `cargo test -p skill-examples` and `cargo test --workspace` to a log; `cargo doc --no-deps -p ryo2d`. They pass.
- [ ] **Step 3: Search for leftovers,** with `--hidden` so `.claude` is searched: `rg --hidden -i "engine_warn|game_warn|game_info|dev_log|system:ip|with_console_commands|CommandContext|\[system\] console|console = (true|false)" --glob '!docs/specs/**' --glob '!docs/plans/**' --glob '!docs/research/**' --glob '!docs/backlog/follow-ups.md' --glob '!.git/**' --glob '!target/**'` finds nothing but `[debug]` `console =` lines.
- [ ] **Step 4: Run the checks, then commit.**
```bash
git add .claude/skills src/core/logging/AGENTS.md src/core/console/AGENTS.md src/core/windows/AGENTS.md src/core/AGENTS.md src/core/config/AGENTS.md src/core/testing/AGENTS.md src/cli/AGENTS.md docs/architecture/overview.md README.md docs/backlog/follow-ups.md
git commit -m "docs: skills, module notes and the overview describe the log, the console and builds without tools"
```
