# Saves and settings: speed and limits

Spec: `docs/specs/2026-11-07-saves-and-settings-design.md`. The bench is `benches/saves.rs` (`cargo bench --bench saves`); its header says what each figure is of.

## Measured

Measured on 2026-11-08 on an Apple M5 Pro (macOS 27.0.1), a shared machine with background load that could not be stopped (macOS photo analysis and a virtual machine). Every run's `uptime` is given, taken just before the run; **all three runs were at a 1-minute load above 3** (6.66, 6.53 and 5.25), so the figures are those of a busy machine, and the run is not the quiet one the plan asked for. I did not wait for quiet. The three runs are `cargo bench --bench saves` in release, one after another; logs: the scratchpad's `bench-saves-1.log` to `bench-saves-3.log`.

| Run | `uptime` just before | Load at the bench's start |
|---|---|---|
| 1 | 19:09, load averages 6.66 5.94 6.23 | 7.60 6.19 6.30 |
| 2 | 19:11, load averages 6.53 6.06 6.25 | 6.53 6.06 6.25 |
| 3 | 19:11, load averages 5.25 5.81 6.15 | 5.25 5.81 6.15 |

**What the bench is.** A headless engine (`HeadlessEngine`), whose saves and `system.json` live in memory (`MemorySaves`). So these figures hold the engine's own work (the slot check, the header, queueing, the encode and deflate on the writer thread, the checks and the inflate on a load) and not a disk's: no file write, `fsync` or rename is in them. The one exception is `frames_during_a_write_disk` (below, measured in a later run), whose engine keeps its saves in a folder on the local disk. The save data is a list of items (an id, a name, a position, a stack) whose RON text is 1,027 B, 100,004 B or 1,000,058 B long, from a fixed pseudo-random sequence.

### The targets

| Target | Run 1 | Run 2 | Run 3 | Verdict |
|---|---|---|---|---|
| `save_handoff_1kb`: the `save` call alone, median of 400 calls, at most 20 µs | 1.46 µs (min 0.83, max 42.38) | 1.50 µs (min 1.00, max 25.75) | 1.46 µs (min 1.00, max 21.00) | **met** in all three |
| `save_handoff_1mb`: the same, median of 100 calls, at most 20 µs | 4.62 µs (min 3.29, max 10.42) | 5.00 µs (min 3.54, max 140.83) | 4.62 µs (min 3.50, max 11.38) | **met** in all three |
| `idle_settings`: `apply_settings` and the writer's tick with nothing changed, median of 100 batches of 10,000 calls, at most 1 µs | 20.0 ns (min 20.0, max 22.0) | 20.0 ns (min 20.0, max 23.0) | 25.0 ns (min 22.0, max 32.0) | **met** in all three |
| `idle_settings`, criterion's median of the same call | 20.9 ns (20.836 to 21.011) | 21.0 ns (20.934 to 21.011) | 20.9 ns (20.805 to 20.936) | agrees with the guard |

How the `save` call was timed: the previous write was finished and drained before each timed call (so the writer thread was idle, and a call never waited on a write queued before it), the data was cloned before the clock started, and the first five calls (which start the writer's thread) were left out. A save queued while the writer is busy with an earlier one was not timed. The saves' clock is `Instant`, which ticks every 41.7 ns on this machine (the minimums read 0.83 µs and 1.00 µs). The maxima are single calls; the largest, 140.83 µs, is one of 100 in run 2, with the median at 5.00 µs.

`idle_settings` goes through `HeadlessEngine::idle_settings` (a `#[doc(hidden)]` hook, which calls `Engine::apply_settings` and `Engine::save_settings` with the clock's now, and says whether the engine has a settings writer; the bench asserts it does) on an engine that keeps every preference.

### Frames during a write (`frames_during_a_write`, recorded)

A headless frame (`HeadlessEngine::step`, 1,000 sprites at 300 x 200, not paced) while a 1 MB save writes, against the same engine with no write. Each round is 30 pairs; a pair is a run of frames from the call to the save until the write was drained (about 117 to 129 frames), and a run of the same number of frames with no write, in alternating order, so a change in the machine's load reaches both. The verdict reads "within noise" when the pooled medians differ by no more than the spread of the no-write medians across the three rounds.

| Run | Round | No write: median (p95), frames | Write: median (p95), frames | Difference |
|---|---|---|---|---|
| 1 | 1 | 196.7 µs (270.7), 3,534 | 197.7 µs (271.5), 3,517 | +1.0 µs |
| 1 | 2 | 185.7 µs (221.0), 3,586 | 185.7 µs (244.7), 3,648 | +0.0 µs |
| 1 | 3 | 186.3 µs (229.1), 3,680 | 185.1 µs (233.3), 3,674 | -1.2 µs |
| 2 | 1 | 183.6 µs (227.5), 3,742 | 180.5 µs (235.0), 3,715 | -3.1 µs |
| 2 | 2 | 188.5 µs (242.4), 3,624 | 191.5 µs (279.1), 3,584 | +3.1 µs |
| 2 | 3 | 192.8 µs (365.1), 3,492 | 197.2 µs (376.9), 3,484 | +4.4 µs |
| 3 | 1 | 181.5 µs (210.6), 3,788 | 179.5 µs (225.0), 3,866 | -2.0 µs |
| 3 | 2 | 180.5 µs (211.7), 3,800 | 176.0 µs (215.0), 3,786 | -4.5 µs |
| 3 | 3 | 182.8 µs (213.2), 3,756 | 178.2 µs (218.9), 3,881 | -4.5 µs |

| Run | Pooled no write | Pooled write | Difference | Spread of the no-write medians | Verdict |
|---|---|---|---|---|---|
| 1 | 188.8 µs | 188.7 µs | -0.1 µs | 11.0 µs | **within noise** |
| 2 | 187.8 µs | 188.8 µs | +0.9 µs | 9.1 µs | **within noise** |
| 3 | 181.5 µs | 177.9 µs | -3.6 µs (-2.0%) | 2.3 µs | **differs by -3.6 µs (-2.0%)** by the rule: the frames with a write queued were the faster ones |

In none of the nine rounds were the frames with a write slower by more than 4.4 µs of about 190, and in run 3 they were faster. I did not measure why run 3's difference is negative, so none is given. A write here is the encode and deflate on the writer's thread into a memory store; a disk's `fsync` is not in it: the disk-backed record below has it.

### Frames during a write on disk (`frames_during_a_write_disk`, recorded)

The same pair loop as `frames_during_a_write` (three rounds of 30 pairs, alternating order, the same verdict rule), with the engine's saves in a temporary folder on the local disk: `HeadlessEngine::save_to_folder` puts a `FileStorage` (the save folder's storage) under the headless engine's saves. The folder was in `/var/folders/.../T/` (`tempfile`), on the Mac's internal SSD (APFS). So each 1 MB save is a native save folder's whole write: the temporary file written and synced (`sync_all`), the current file read and checked, renamed to the backup, the temporary file renamed over it, the folder synced and the leftovers cleared. The save file is 298,900 B.

One run, `cargo bench --bench saves` in release, on 2026-11-08 on the same Apple M5 Pro (macOS 27.0.1). `uptime` just before it: 19:53, load averages **7.15** 10.06 8.87, so **the 1-minute load was above 3** (the background load could not be stopped) and this is a busy machine's run. Log: the scratchpad's `bench-saves-disk.log`.

| Round | No write: median (p95), frames | Write on disk: median (p95), frames | Difference |
|---|---|---|---|
| 1 | 180.2 µs (215.5), 4,782 | 180.9 µs (224.7), 4,800 | +0.7 µs |
| 2 | 180.1 µs (215.8), 4,778 | 181.1 µs (226.6), 4,784 | +1.0 µs |
| 3 | 181.2 µs (217.5), 4,776 | 181.5 µs (227.5), 4,777 | +0.3 µs |

| Pooled no write | Pooled write on disk | Difference | Spread of the no-write medians | Verdict |
|---|---|---|---|---|
| 180.5 µs | 181.2 µs | +0.7 µs (+0.4%) | 1.2 µs | **within noise** |

**The native-disk case met the target** ("a 1 MB save writing in the background changes frame time by no more than noise, by interleaved A/B") by the bench's rule, in this one run at a 1-minute load of 7.15. What else the run shows: the frames with a write were the slower ones by the median in all three rounds (by 0.3 to 1.0 µs of about 181), each inside the spread, and their p95 was 9.2 to 10.8 µs higher in each round. A pair ran about 159 to 160 frames from the save until its write was drained, against about 123 to 125 for the memory store in the same run. I did not measure the causes of these differences, so none is given.

The same run's memory-store record, for comparison: pooled no write 181.3 µs, write 181.1 µs, difference -0.2 µs, spread 0.8 µs, within noise (rounds -0.8, +0.7 and -0.4 µs). Its guards: `save_handoff_1kb` 1.46 µs, `save_handoff_1mb` 4.42 µs and `idle_settings` 20.0 ns (criterion 20.9 ns), all met; `load_100kb` 965.1 µs and `load_1mb` 10,480.5 µs.

### Loads and the file's size (`load_100kb`, `load_1mb`, recorded)

`load` of a saved slot, a median of 100 loads (the first three left out), data read back and checked for its length:

| Data (RON text) | Run 1 | Run 2 | Run 3 | The save file |
|---|---|---|---|---|
| `load_100kb` (100,004 B) | 1,058.1 µs (min 951.0, max 1,229.6) | 1,055.6 µs (min 993.4, max 1,361.1) | 1,011.1 µs (min 977.3, max 1,175.4) | 30,529 B |
| `load_1mb` (1,000,058 B) | 10,986.7 µs (min 10,356.3, max 12,362.1) | 10,910.0 µs (min 10,441.3, max 11,553.5) | 10,930.6 µs (min 10,570.4, max 12,063.2) | 298,900 B |

The file is about 30% of the RON text for this data. A load is on the calling thread; a load of a queued save waits for its encoded bytes, which these did not (each was settled first). No target.

### Changes the bench made to the engine

Two hooks, both `#[doc(hidden)]` in `src/core/testing/headless.rs`: `HeadlessEngine::idle_settings`, and `HeadlessEngine::save_to_folder(dir)`, which puts a `FileStorage` on `dir` under the engine's saves (and its settings file) for the disk record.

## Limits

Each was checked against the code or the module notes (saves and settings `AGENTS.md`, tile maps' `AGENTS.md`, `src/core/saves/idb.rs`, `src/core/tilemap/stream.rs`) before it was written here.

- **On the web a save's serialization runs on the frame** that calls `save`: the writer has no thread there and runs its jobs where they are sent, so a save serializes, encodes and writes on that frame, and saves of one slot are not coalesced (a game saving every frame pays every frame). Keep web saves small, or save at quiet moments. The bench's figures are native: the `save` call's 1.5 to 5 µs is the hand-off to a thread, and a web save's whole cost on the frame was not measured.
- **Closing a browser tab right after a save may lose the write:** browsers do not wait for IndexedDB commits at unload. `SaveFinished` arrives once the commit has landed.
- **The web's first scene waits up to 2 s for the browser's database to open, and then for every file in it to be read, with no time limit on the read**: `open_in_time` races only the database's open against the 2 s timer (`open_then_read`). A database that does not open in time starts the game with saves `Unavailable` and settings lasting only the session, with one warning; once it has opened, a store that holds a lot delays the first scene by as long as its read takes, and a read that never finishes would hold the first scene. Neither wait was measured.
- **A saved fullscreen is not requested when a web game loads** (browsers enter fullscreen only from the player's gesture; the page sizes the canvas, so the window size is not kept either), and **vsync is a no-op on the web.**
- **The native guarantees assume a local disk:** the crash safety rests on atomic renames and on `fsync` of the file and its folder, which a network folder can make slow or not atomic. The bench's only disk figures (`frames_during_a_write_disk`) are of one local SSD.
- **A settings value equal to the default at save time leaves the file,** so a default changed later (in Ryo.toml or on the builder) moves a setting the player had set to the old default; a player who set a value equal to the default has not set it, as far as the file knows.
- **Every backup rotation reads and verifies the current file:** a replace reads the current copy and checks it before renaming it to the backup (a damaged copy never replaces a good backup), so a save of a large slot pays a read and a checksum of the old file as well as the write. It is inside the disk record's writes, which were not timed on their own.
- **Two copies of a game saving the same slot at once can fail each other's saves** (one's cleanup of leftover temporary files removes the other's before its rename; the failed save reports an `Io` error), but the folder always ends with a verified save and no temporary files.
- **On the web, `Ryo.toml`'s `[env]` has no reading API beyond parsing `ProjectFile::text()`:** `Workspace` and its `env_*` getters are native only (`Workspace::discover()` is an error there, and `Workspace::from_toml_str` is crate-internal), so a web game that wants `[env]` parses the text itself.
- **Saving a tile map's `SavedChunks` clone through `ctx.saves()` serializes the shared store under its lock on native:** the clone shares the store, and the save job (on the writer's thread) holds the lock while it serializes, so a chunk unloading in that moment waits for it. On the web the serialization happens at the call.
- **The figures above are for a busy machine, and all but one record for a memory store.** Run at a 1-minute load of 5 to 7.6 (the disk record's run at 7.15), so a quiet machine's figures are not recorded; a real disk's cost reaches only `frames_during_a_write_disk`'s frames (one run, one local SSD), and the bench cannot show the web's.
