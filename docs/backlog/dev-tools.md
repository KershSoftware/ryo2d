# Dev tools: speed and limits

## Measured

### Before (main ad022a3e)

Machine: the owner's Mac (Darwin, shared), `benches/dev_tools.rs`, run with
`cargo bench --bench dev_tools`. `log_call_warn` is measured with the log's
terminal output switched off (`dev_log::set_stdout_enabled(false)`), not
redirected to the null device, because the bench prints its own results on the
terminal; so it holds the formatting, the lock and the buffer, not a terminal
write. `log_call_quiet` is measured the same way.

One-minute load before each run (`uptime`), all above 3 and so flagged: the
machine was busy with other work.

| Run | Load averages (1, 5, 15 min) | Flag |
|-----|------------------------------|------|
| 1 | 5.99, 4.21, 3.23 | load above 3 |
| 2 | 5.99, 4.21, 3.23 | load above 3 |
| 3 | 5.59, 4.16, 3.22 | load above 3 |

(Runs 1 and 2 started within the same minute, so `uptime` showed the same
figures.)

| Bench | Run 1 | Run 2 | Run 3 |
|-------|-------|-------|-------|
| `log_call_quiet` (ns a call, 1,000,000 calls) | 116.7 | 102.6 | 103.2 |
| `log_call_warn` (ns a call, 100,000 calls, terminal off) | 98.3 | 99.9 | 105.0 |
| `console_open_idle`, median over the closed frame (us; closed frame median 82.6, 81.8, 83.7) | 187.6 | 184.1 | 188.9 |
| `console_open_idle`, 95th percentile (us) | 233.1 | 225.0 | 275.5 |

`console_open_idle`: 1,000 lines of 80 characters, 1920 x 1080, 1,000
interleaved frames of each engine.

Release binary, `cargo build --release -p console-demo`
(`target/release/console-demo`): 42,049,712 bytes. `strings` finds
`system:timescale` (3 matching lines) and `Fetching WAN IP` (1 matching line),
so both are in the build today.

### After (branch `dev-tools`)

Same machine and bench (`benches/dev_tools.rs`), run three times without the
tools (`cargo bench --bench dev_tools`) and three times with them
(`cargo bench --bench dev_tools --features dev-tools`). Every log call is made
with an engine target, as the earlier figures' were. The terminal output is
off, as before. The one-minute load before each run (`uptime`) was above 3 in
every run, so every figure here is flagged: the machine was busy with other
work (an unrelated build and a browser). The figures are the median of the
three runs, with the spread in brackets.

| Bench | Before | After, without tools | After, with tools | Target or guard | Verdict | Load at each |
|-------|--------|----------------------|-------------------|-----------------|---------|--------------|
| `log_call_quiet` (ns a call) | 103.2 (102.6 to 116.7) | 0.7 (0.6 to 0.9) | 0.9 (0.6 to 1.2) | 5 ns | met | Before 5.6 to 6.0; after 7.3 (no tools), 7.1 (tools) |
| `log_call_warn` (ns a call) | 99.9 (98.3 to 105.0) | 69.5 (67.7 to 73.5) | 119.8 (114.6 to 139.9) | no slower than Before's median, 99.9 ns | met without tools; **missed with tools** | as above |
| `console_open_idle` (us over the closed frame) | 187.6 (184.1 to 188.9) | no console | 4.6 (4.0 to 4.7) | under a quarter of Before: 46.9 us | met | Before 5.6 to 6.0; after 7.1 |
| `console_open_idle`, 95th percentile (us) | 233.1 (225.0 to 275.5) | no console | 58.2 (58.1 to 73.9) | none | recorded | as above |
| `tools_closed` (ns a frame, the tools' own time, closed) | not measured | no tools | 0 to 41 (every run's median was 0 or one clock tick, 41 ns; 95th percentile 42) | 1 us | met | 7.1 |

Notes on the figures:

- `log_call_quiet` is not the same work as Before's: the earlier log stored
  its debug lines, and this one cuts the record at the level check, so
  After's figure is what a game pays for a silenced line (one atomic read in
  the macro and a call).
- `log_call_warn` goes, in a bench, to the file output's early buffer (each
  record cloned into its 256-record buffer under a mutex), because opening a
  real log file in a temp folder is crate-internal and was not given a hook.
  With the tools it also goes to the ring. The miss with tools is the ring
  and what comes with it (read from the code, not profiled): a second copy of the record (two strings allocated),
  the ring's lock, a map update for the scope's warning count (a hashed
  lookup), and, because the bench fills the ring, the oldest record's two
  strings freed on every push. Without the ring the figure is 30 ns under
  Before's; with it, about 20 ns over. The old log's buffer did less per
  record. The target was not loosened.
- `console_open_idle` is the open console's frame over a closed one, with
  1,000 lines, at 1920 x 1080: a frame costs about 5 us more open than closed,
  where the earlier console cost about 187 us more. The closed frame's own
  median was 85 to 113 us (it varies with the load).
- `tools_closed` is read from a timer around the console's tick in the
  engine's frame (`HeadlessEngine::tools_time`). The clock ticks every 41.7 ns
  here, so a median of 0 means under one tick. The dev windows are drawn by
  the window loop, not the frame, and a headless engine opens none, so their
  closed cost (two option checks a frame) is not in it.

Release binary of `console-demo`, `cargo build --release -p console-demo`
(the demo's own code changed since Before, so the first size is compared with
Before's only roughly), and the same with `--features ryo2d/dev-tools`:

| Build | Bytes | `system:timescale` | `Welcome to Ryo2D Developer Console` | `core::console::Console` | `Fetching WAN IP` | `Unknown command` |
|-------|-------|--------------------|--------------------------------------|--------------------------|-------------------|-------------------|
| Before (`console-demo`) | 42,049,712 | present | not checked | not checked | present | not checked |
| After, no tools | 41,945,856 | absent | absent | absent | absent | present (1: the demo's own text) |
| After, with tools | 42,375,440 | present (2) | present | present | absent | present (3) |

A build without the tools has none of the engine's commands or console in it;
the tools add about 430 kB.

## Limits

- The dev windows (the logger and the profiler) are native only, and the
  profiler is not on the web.
- `tracing` output (winit's) is not read by the log.
- A game calling `log::info!` directly (not through the prelude's macros)
  counts as a library, with the library's default level.
- A log line from a library before the first prelude macro or engine has
  installed the logger is lost.
- The per-system "warn once" sets are not shared: each system warns once on
  its own.
- The log file is flushed at most every 50 ms, so up to 50 ms of warnings can
  be lost in a hard crash that skips the panic hook.
- A scene's command cannot be typed while its scene is off the stack.
- Records logged on a library's own thread (the audio and gamepad libraries')
  carry no engine scope, so a test's capture and a second engine's console do
  not see them.
- `log:level` and `RYO_LOG` treat `game`, `engine` and `library` as the three
  defaults, so a crate whose target is exactly `engine` or `library` cannot be
  set at its root (its sub-targets can).
- `ryo2d dev -p` passes `--features ryo2d/profiler`, so a game that renames
  its `ryo2d` dependency cannot use `-p`.
- A game's own command names and descriptions stay in a shipped binary (the
  engine's do not).
- On macOS and Linux a second copy of a running game takes over `latest.log`
  (the first copy's file is renamed to `previous.log` under it); the
  per-process fallback name (`latest-<pid>.log`) only triggers on Windows.
- A builder command replaces a built-in of the same name for the whole run.
- The console's count of warnings no longer shown is of warnings and errors,
  whatever `log:show` is set to.
- An open console costs about 27 allocations a frame in the UI's own
  recording of what it draws.
