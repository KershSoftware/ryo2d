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
