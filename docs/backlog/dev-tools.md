# Dev tools: speed and limits

## Measured

### Before (main ad022a3e)

Machine: the owner's Mac (Darwin, shared), `benches/dev_tools.rs`, run with
`cargo bench --bench dev_tools`. `log_call_warn` is measured with the log's
terminal output switched off (the earlier log's own stdout switch), not
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

The two `log_call_warn` rows were measured again, three runs each way, after
the log's path for a recorded line was reworked (below), with a log file open
as a running game has one. The machine was busy then too, with another
project's emulator tests (load 7.0 to 7.5, flagged). The other rows are from
the first set of runs.

| Bench | Before | After, without tools | After, with tools | Target or guard | Verdict | Load at each |
|-------|--------|----------------------|-------------------|-----------------|---------|--------------|
| `log_call_quiet` (ns a call) | 103.2 (102.6 to 116.7) | 0.7 (0.6 to 0.9) | 0.9 (0.6 to 1.2) | 5 ns | met | Before 5.6 to 6.0; after 7.3 (no tools), 7.1 (tools) |
| `log_call_warn` (ns a call, a log file open) | 99.9 (98.3 to 105.0) | 76.2 (67.7 to 76.6) | 91.4 (89.2 to 93.4) | no slower than Before's median, 99.9 ns | met in all six runs (worst 94.8 ns, in an earlier set's runs with the tools: 91.7 to 94.8 ns at a load of 4.4 to 4.8); with the tools by 5 to 10 ns, which is less than Before's own run-to-run spread: re-check on a quiet machine | Before 5.6 to 6.0; after 7.0 (no tools), 7.5 (tools) |
| `log_call_quiet_prelude` (ns a call, the prelude's `debug!`) | not measured | 1.7 | 1.4 | 5 ns | met | 5.9 (no tools), 3.4 (tools) |
| `log_call_warn_no_file` (ns a call, no file open yet) | 99.9 (Before wrote no file, so this row is the like-for-like comparison) | 61.4 (58.2 to 65.7) | 69.3 (68.3 to 78.7) | none | recorded | after 7.0 (no tools), 7.5 (tools) |
| `console_open_idle` (us over the closed frame) | 187.6 (184.1 to 188.9) | no console | 4.6 (4.0 to 4.7) | under a quarter of Before: 46.9 us | met | Before 5.6 to 6.0; after 7.1 |
| `console_open_idle`, 95th percentile (us) | 233.1 (225.0 to 275.5) | no console | 58.2 (58.1 to 73.9) | none | recorded | as above |
| `tools_closed` (ns a frame, the tools' own time, closed) | not measured | no tools | 0 to 41 (every run's median was 0 or one clock tick, 41 ns; 95th percentile 42) | 1 us | met | 7.1 |

After the final fixes (the panic hook's record kept from stderr, the file
written before stderr, the file's open, the early buffer's note of what
stderr took), the log benches were run once each way, with the new
`log_call_quiet_prelude` (one `ryo2d::debug!`, the prelude's macro a game
calls, which also calls `logging::ensure()` before `log`'s level check; the
other rows time `log`'s macros with an engine target). The machine was
running a virtual machine throughout, which a few minutes of checks did not
avoid: the run without the tools started at a load of 5.9 (3.6 just before
its build), the run with them at 3.4, both flagged. Every guard was met:

| Bench (final fixes, one run each) | Without tools | With tools | Target or guard | Verdict |
|-------|---------------|------------|-----------------|---------|
| `log_call_quiet` (ns a call) | 1.2 | 0.6 | 5 ns | met |
| `log_call_quiet_prelude` (ns a call) | 1.7 | 1.4 | 5 ns | met |
| `log_call_warn` (ns a call, a log file open) | 67.4 | 86.9 | no slower than Before's median, 99.9 ns | met |
| `log_call_warn_no_file` (ns a call) | 60.4 | 69.2 | none | recorded |
| `console_open_idle` (us over the closed frame) | no console | 4.6 (95th percentile 65.9) | under 46.9 us | met |
| `tools_closed` (ns a frame) | no tools | 0 (95th percentile 42) | 1 us | met |

Notes on the figures:

- `log_call_quiet` is not the same work as Before's: the earlier log stored
  its debug lines, and this one cuts the record at the level check, so
  After's figure is what a game pays for a silenced line (one atomic read in
  the macro and a call).
- `log_call_warn` is measured with a log file open in a temp folder (opened
  and closed through `ryo2d::testing::open_log_file` and `close_log_file`, a
  fresh file each run, so no run reaches the 8 MiB cap), as a running game
  has: with the tools the warning goes to the file and the ring, without them
  to the file alone. `log_call_warn_no_file` is the same call before any file
  opens, when the file output keeps a copy of each record in its 256-record
  early buffer.
- Where a logged warning's time goes now, with the tools and a file open
  (measured piece by piece): the level check about 8 ns; the record itself
  about 30 ns (its clock read is 17 of them, the rest writing its message and
  target into reused buffers); the file's line about 32 ns (16 writing the
  text, the rest its lock and the buffered write); the ring about 10 ns (its
  lock and moving the record in). The rest is the `log` crate's call. Without
  the tools there is no ring. The earlier log wrote no file, so the file's
  line is work it did not do; the no-file figure is the like-for-like one.
- Before the rework, the same call with a file open cost about 258 ns with
  the tools and 227 without, and 120 with the tools and no file (the first
  set's miss). The waste was the file line written with the formatter (140
  ns, now 16), a second clock read for the file's 50 ms flush gap (it now
  reads the record's own time), an 8 KiB file buffer that paid a write call
  every couple of hundred lines (now 64 KiB: the flushes bound how long a
  line waits while frames run, 50 ms; in a flood the buffer bounds how much
  waits, 64 KiB; if the game hangs, everything since the last flush waits),
  and a copy of the record for each output that kept it, freed again
  later. Now one record moves into the ring, and the one the full ring drops
  lends its buffers to the thread's next record, so a steady stream of
  warnings allocates and frees nothing.
- An earlier set of runs on the same code, but for one change (a game's
  warning counts in the ring kept out of a map, a few nanoseconds), at a load
  of 4.4 to 5.5: `log_call_warn` 65.5 ns without the tools and 93.3 with;
  `log_call_warn_no_file` 56.6 and 71.7.
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
- Lines not yet flushed to the log file are lost in a hard crash or kill that
  skips the panic hook: while frames run, at most the last 50 ms (and at most
  64 KiB during a flood); if the game hangs, everything since the last flush.
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
- Each copy of a running game writes its own log file: the first holds
  `latest.log` (an advisory lock on macOS and Linux; on Windows, opened
  without sharing deletion, so it stays readable), and a second copy started
  meanwhile leaves it and `previous.log` alone and writes
  `latest-<pid>.log`; a later start removes the pid files no running copy
  holds. Two copies started in the same instant can still race for
  `latest.log`.
- A builder command replaces a built-in of the same name for the whole run.
- The console's count of warnings no longer shown is of warnings and errors,
  whatever `log:show` is set to.
- An open console costs about 27 allocations a frame in the UI's own
  recording of what it draws.
