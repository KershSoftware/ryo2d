# AGENTS.md - Saves Module

Game saves, from `ctx.saves()` down to the bytes on disk. A game saves a value in a slot and loads it back; the save service checks and queues the save on the main thread and encodes and writes it on a thread of its own, in call order, and a load sees what is queued. The save file carries a header, checksums and the game's data version, and older data upgrades through typed steps. The storage layer keeps files whole: a crash at any point leaves a whole, verified copy of either the old or the new bytes, and recovery finds it. The design is `docs/specs/2026-11-07-saves-and-settings-design.md` (where it and the code differ, the code is current).

| File | What |
|------|------|
| `mod.rs` | The exports: `SavesView`, `Saves`, `SaveInfo`, `Loaded`, `SaveSlot`, `SaveFinished`, `SaveOp`, `SaveError`, `SaveHeader`, `SaveVersions` and `MemorySaves` are public (all but `MemorySaves` in the prelude; `ryo2d::testing` re-exports it); the quit flush's two durations (3 s to warn, 10 s to give up); the rest is crate-internal |
| `view.rs` | What a game uses: `SavesView` (`ctx.saves()`), the `Saves` system parameter, `SaveInfo`, `Loaded`, `SaveSlot`, `SaveFinished` and `SaveOp`, and the `drain_save_results` system |
| `service.rs` | `SaveService`, the resource behind both views: the storage, the versions, the queued saves and deletes, the writer and its results (`Finished`) |
| `writer.rs` | `Writer`, the one thread the service's jobs run on, in order, with the files still being written and a wait for them |
| `error.rs` | `SaveError`, a `thiserror` enum whose causes are its `source()` |
| `container.rs` | The save file: `SaveHeader`, `encode`, `read_header`, `verify`, `decode` (to a `Body`), `peek_format`; format 1 read as well. Its tests' `fixtures` module holds the format-1 fixture and its `LegacyFixture` type for other tests |
| `versions.rs` | `SaveVersions<T>` (a type's current version and its steps) and `VersionRegistry` (by type: `register`, `current`, `decode_ron`, `decode_json`) |
| `slot.rs` | The slot rule (`check_new`), older names still reachable (`is_legacy`), and the file names: `file_name`, `backup_name`, `temp_name`, `is_temp_of` |
| `storage.rs` | The seam: `FileOps`, `Storage` (implemented for every `FileOps` backend), `Completion`, `Recovery`, `Origin` |
| `replace.rs` | The order of writing, recovering, removing and setting aside a file (`safe_replace`, `recover`, `remove_all`, `set_aside`) |
| `files.rs` | `FileStorage`, a native folder (not on the web) |
| `memory.rs` | `MemorySaves`, the in-memory store, with its faults for tests (`FaultOp`) |

## The save file

Format 2, every integer a little-endian u32:

| Bytes | Content |
|---|---|
| 0-4 | `RYO2` |
| 4-8 | the format, 2 |
| 8-12 | the header's length H |
| 12-16 | the body's length B |
| 16-20 | the header's CRC-32 |
| 20-24 | the body's CRC-32 |
| 24 on | the header (`SaveHeader` as JSON), then the body (RON deflated at level 6) |

The CRC is `flate2::Crc`. The header holds the format, the data version, the save time (Unix seconds), the play time, the name, the labels and the engine version; `encode` writes it with format 2 whatever the header says, and a read takes the format from the prefix. The body is the RON of the data itself.

- `read_header` checks the magic, the format, the lengths up to the header and the header's CRC only, so a slot list reads a save whose body is cut off or damaged.
- `verify` checks the whole file (the exact length and both CRCs; format 1 by parsing all of it) and returns the save time, as `Storage`'s `verify` wants. It is `None` for a file of a newer format.
- `decode` checks the whole file and inflates the body.
- `peek_format` is the format of any file with the magic, newer ones included, so a newer save can be refused as `NewerVersion` rather than taken for damage.
- Errors: no magic (or format 0) is `Corrupt { reason: "not a Ryo2D save" }`; a format above 2 is `NewerVersion { found, current: 2 }`; anything else wrong is `Corrupt` with its reason. The container does not know the slot, so `Corrupt`'s `slot` is empty for the caller to fill.

Format 1, the old save manager's, is `RYO2`, 1, then the deflated RON of `GameSaveData<T>`: `(data: .., timestamp, name, play_time, metadata)`. `read_header` parses everything but `data` (`IgnoredAny`) into a header with format 1, version 1, the labels from `metadata` and an empty engine version. `decode` returns the whole RON with `Body::legacy` set; the text is taken lossily, as the old reader did. Format 1 has no checksum: a cut-off file fails the RON parse.

`tests/fixtures/saves/` holds a format-1 save written by the old code, and `system.json` at formats 1 and 2; its README says how they were made. Never regenerate them.

## Versions

A game gives each versioned type a `SaveVersions<T>`: `new(current)`, then `step(from, |old: A| -> B)` for every version from 1 to `current - 1`. A type never registered is version 1, as is every format-1 save.

- Each step keeps two conversions built from the one closure: RON text to RON text (`ron::from_str` of A, then `ron::to_string` of B) for saves, and `serde_json::Value` to `Value` for settings sections. Data passes between steps serialized, so a step's B and the next step's A need only agree on their shape.
- `VersionRegistry::register` panics, naming the type, when `current` is 0, on two steps from one version, on a step outside 1 to `current - 1` and on a missing step (`"SaveVersions<{type}>: missing step from version {n}"`). So a gap fails as the types are registered, never at a player's load. A second registration of a type replaces the first.
- `decode_ron::<T>(version, &body)` runs the steps from `version` and returns the value and `Some(version)` when it upgraded. A legacy body is unwrapped through a typed `{ data: A }` wrapper by the first step it meets, or by the final read when the type is still version 1. A version above the current one is `NewerVersion`, version 0 is `Corrupt`, and data that does not read (in a step, as `StepFailed` naming the step, or at the end) is `Deserialize` with an empty slot.
- `decode_json::<T>(key, version, value)` is the same for a settings section, and its errors name `key` as the slot.

## Slot names

A new save's slot is 1 to 64 characters of `a`-`z`, `0`-`9`, `-` and `_`, and not a Windows device name (`con`, `prn`, `aux`, `nul`, `com1`-`com9`, `lpt1`-`lpt9`), so one name is one slot on case-sensitive and case-insensitive disks and as a browser key. A name the old save manager accepted (non-empty, no `/` or `\`, no `..`, no leading `.`) that the rule refuses is legacy: it can still be loaded, listed and deleted on native, never written.

## Files

- A slot is kept in `<slot>.ryo-save`. The storage itself is flat and takes any file name (settings keep `system.json` in the same store).
- The backup is `<name>.bak`, the previous copy, made by the last replace.
- A temporary file is `<name>.<pid>-<n>.tmp`: the process id and a process-wide counter, so a game launched twice never writes the same temporary file. The web has no process ids and uses 0. `is_temp_of` matches exactly this shape, so one file's temporaries are never taken for another's (`a.ryo-save.1-0.tmp` is not a temporary of `a`).

## The storage seam

`Storage` is what the saves use: `read`, `names`, `replace`, `recover`, `remove_all` and `set_aside`. Writes, removals and renames return a `Completion` (`#[must_use]`): `Done` with the result, or `Pending` with a channel the result arrives on, for a backend that commits asynchronously (the browser's). `Completion::poll` checks without blocking; `wait_until(deadline)` waits until the deadline on native and polls once on the web, which cannot block; `wait` blocks and only tests have it. A pending completion whose sender is dropped is an error.

A folder-like backend implements only `FileOps` (`read`, `write_synced`, `rename`, `remove`, `names`, `sync_dir`) and gets `Storage` from the blanket impl, which runs the algorithms in `replace.rs`. `FileOps` and `Storage` share the names `read` and `names`, so code with both traits in scope calls them as `FileOps::read(..)` or `Storage::read(..)`.

`verify` is the caller's check of one copy: `Some(recency)` for a whole, valid copy, where a larger recency is a newer save, and `None` otherwise. The storage never parses bytes itself.

## Replace

`safe_replace(ops, name, bytes, verify)`:

1. writes the temporary file and flushes it to disk (`write_synced`);
2. renames the current file to the backup, but only if it reads and verifies: a damaged current file never replaces a good backup. A current file gone since it was read (another writer moved it) is passed over;
3. renames the temporary file over the current one;
4. syncs the folder; a failure warns once per process and still succeeds, since the data is in place;
5. removes every other temporary file of this name: leftovers of a crash, or of another writer.

Both renames are retried on `PermissionDenied` (and Windows' sharing violation) up to 10 times, 10 ms apart: a virus scanner or an indexer may hold a fresh file for a moment. A failed replace removes its own temporary file (a removal that fails is cleared by the next save), so the old copy stays the one found.

The invariant at each step: until step 2 the current file is the old copy; between steps 2 and 3 the old copy is the backup and the new one a whole, flushed temporary file; after step 3 the current file is the new copy. Every rename is atomic, so there is no instant without a whole copy.

Two writers on one folder (a game launched twice) can fail each other's saves: one's step 5 removes the other's temporary file before its rename. The folder still ends with a verified save and no temporary files.

## Recover

`recover(ops, name, verify)` prefers the current file if it verifies, then the verified temporary file with the highest recency (a crash between steps 2 and 3 leaves the new copy there), then the backup if it verifies (`Origin` says which). When the current file or the backup is there but nothing verifies, it is `Damaged`; with neither there, `Missing`, even beside a torn temporary file, which is no save. A temporary file that cannot be read (another writer clearing it) is passed over; an error reading the current file or the backup is returned.

## Remove and set aside

- `remove_all` removes the temporary files, then the backup, syncs the folder, then removes the file and syncs again: a removal cut short leaves the whole save or nothing, never a backup or temporary file that would come back as the save.
- `set_aside(name, to)` renames a file over `to` (with the same retry), as settings do with a damaged file.

## The backends

- `FileStorage::new(dir)` creates the folder. `write_synced` truncates and `sync_all`s; `sync_dir` opens the folder and `sync_all`s it on Unix, and does nothing on Windows. `names` lists files only, sorted, skipping names that are not UTF-8. Native only.
- `MemorySaves` is a `Clone` handle on shared state (`Arc<Mutex<..>>`): `put`, `get`, `names` and `remove` reach the files directly and are not counted as operations. Its faults exist in the crate's tests only (`#[cfg(test)]`):
  - `fail(op, nth)` fails the nth call of `op` and `fail_from(op, nth)` every call from the nth on, both 0-based and counted from when they are set, for `FaultOp::{Write, Rename, Remove, SyncDir}`;
  - `crash_write(nth, keep)` stores only the first `keep` bytes of the nth write, then fails;
  - `deny_rename(times)` fails the next renames with `PermissionDenied`;
  - `slow(per_op)` makes every operation wait (on native; the web cannot block);
  - `ops()` counts every `FileOps` call and `writes()` every write, failed ones included.

## The service

`SaveService` is a world resource, made in `Engine::new` before the game's own resources and systems. It is internally synchronized: every method takes `&self`, so `SavesView` (`ctx.saves()`, over `&World`) and `Saves` (a `SystemParam` over `Option<Res<SaveService>>`) only read it, and systems that save run beside each other. A world without it (one the engine did not make) gives `Unavailable` from both views.

- **Storage.** `set_storage` installs it (the browser's arrives after start-up); until then, and in a build with neither a project nor memory saves, a save is `Unavailable`. `Engine::new` picks a builder's `memory_saves` store in any build, else a new `MemorySaves` when headless, else a `FileStorage` at the project's `Workspace::save_location()` (native; a folder that cannot be made warns and leaves saves unavailable). The web has no workspace, so it has no storage at start.
- **Versions.** `EngineBuilder::save_versions::<T>` registers into the builder's `VersionRegistry` at the call, so a gap panics there; the registry moves into the service.
- **Queued saves.** Each slot has at most one entry, by file name: its latest queued save or delete, with a generation from one counter. A save checks the slot (`check_new`), takes the storage and the type's version, builds the header on the main thread (`saved_at` from `web_time::SystemTime`, `engine_version` from `CARGO_PKG_VERSION`), records the entry and queues a job that holds the data, so serializing leaves the frame too.
- **The save job:** if a later generation of the slot (a save or a delete) is queued it does nothing and reports `Ok` (coalescing: a game saving one slot every frame writes only as fast as the store allows, and each skipped save's data is dropped at once); if the stored file was written by a newer engine (`peek_format` above 2) it refuses with `NewerVersion` and leaves the file alone; otherwise it serializes (RON) and encodes, publishes the bytes for loads, and replaces the file (`container::verify` as the check). Format 1 becomes format 2 at the slot's next save, since a save always writes format 2.
- **Loads.** A slot with a queued save waits for that save's encoded bytes (not the disk) and decodes them; a queued delete is `NotFound`. A save whose job fails (its data does not serialize, or a newer engine's file is in the way) publishes `Failed`, and the load reads the store: the previous save stands. A job that panics publishes `Failed` as it unwinds, and one superseded publishes `Superseded`, so the load looks again at the slot's newer entry. From the store: a current file of a newer format is `NewerVersion`, never recovered from its backup; otherwise `recover`: `Missing` is `NotFound`, `Damaged` is `Corrupt { slot }`, and a copy from the backup sets `from_backup` and warns once per slot. Then `container::decode` and `decode_ron::<T>`; the slot is filled into the `Corrupt` and `Deserialize` errors they leave unnamed.
- **Names.** Writing takes only `check_new` names. Loading, `info`, `exists`, `list` and `delete` also take a legacy name on native, when a copy of that file (itself, its backup or a temporary file) is kept.
- **`info`, `list` and `exists`** read only headers: a queued save's header (unless its job failed), else the newest stored copy whose header reads (`recover` with `read_header` as the check; a current file of a newer format is `NewerVersion` for `info` and left out of the list). `list` finds slots from every stored name that is a save, its backup or one of its temporaries, adds the queued saves, drops the queued deletes, leaves out a slot with no header to show, and sorts by slot (not by file name, which puts `a-b.ryo-save` before `a.ryo-save`). `exists` is a queued save, or any stored copy.
- **`delete`** queues `remove_all` behind the slot's earlier jobs, with its own generation, so a save queued before it is skipped. A delete itself is never skipped, even with a later save of the slot queued: otherwise that save's replace would keep the deleted save as its backup.
- **`is_writing`** is true while the slot has an entry: from the call until `drain` takes its job's result.
- **Named files.** `write_named(name, bytes, verify)` queues a crash-safe replace of any file (the settings' `system.json`) on the same writer, in order with the saves (never coalesced), and returns the job's id. `recover_named` (with no storage, `Missing`) and `set_aside_named` reach the storage directly.

## The writer

`Writer<T>` follows the tile maps' `Worker`: one thread (`"ryo2d-saves"`) started with the first job, a FIFO channel, jobs run under `catch_unwind` (a panic comes back as `None`), and where no thread starts (the web) a job runs where it is sent, so its result is there at once. Unlike the maps' worker, the thread keeps running the queued jobs after no one reads the results, so a save queued before the engine went still lands. It keeps the files of the jobs sent and not finished (`unfinished`), and `wait_idle(deadline)` waits on a condition variable until they are done or the deadline passes.

The service sends a job and records what it is for (`JobKind`: a slot's save or delete, or a named file; its file and generation) under its results lock, so a result is never seen before its job is known.

## Results

- **`drain`** moves the writer's results into `Finished` (`id`, `kind`, `result: Result<(), Arc<SaveError>>`), mapping an `io::Error` to `Io { path: file }`, or to `StorageFull` for `ErrorKind::StorageFull`, and a panicked job to `Serialize`. A job whose storage returned a `Pending` completion is kept and polled with `try_recv` at each later `drain`; it is reported once it resolves, never waited on. A slot's entry is cleared when the result of its latest generation is drained.
- **`drain_save_results`** (`Phase::PreUpdate`) sends a `SaveFinished { slot, op, result }` for each slot job's result (`Finished::message`). A named file's result has no message: `system.json`'s goes to the settings writer (`SettingsWriter::finished`, settings module notes), which gives a `SettingsSaveFailed` for the system to send when it failed; another name's is dropped.
- **`flush(warn_after, give_up)`** waits for the writer to go idle, warning after `warn_after` with the files still being written; at `give_up` it stops waiting, then waits for the pending completions until the same deadline (`wait_until`: polled once on the web), logs the files still unfinished and returns them. Its results stay for the next `drain`. Once a flush has given up, a later one waits no more and warns no more; once everything is written, one returns at once.
- **The quit flush** is `Engine::flush_saves` (`flush(3 s, 10 s)`), run from `ApplicationHandler::exiting` (before the capture flush; macOS's Cmd+Q exits right after it), at the end of `run_offscreen_frames`, and from `Drop for Engine`, so a dropped `HeadlessEngine` finishes its saves too. Each runs right after `Engine::flush_settings`, which queues a pending settings write, so the flush waits for that too.

## Testing

`service.rs`'s tests run the service over `MemorySaves` with no engine: a load of a queued save (on a slow store), two saves of one slot (a named write holds the writer meanwhile, so both are queued: one write, two results), a delete after a save, a save after a delete (no backup of the deleted save), the flush finishing and giving up, 1,000 saves of one slot writing at most 10 times, a store that refuses every write (each save reports `Io` naming the file, the first still loads), a load as another type (`Deserialize` naming the slot), data whose `Serialize` fails, a damaged save loading its backup with one warning and then `Corrupt`, a newer engine's save refused by load, `info` and save, older names on a real folder, the format-1 fixture (rewritten as format 2 at its next save), the list with its headers, a registered type's upgrade, the named-file calls, and a storage whose commits land later (reported once they do, `StorageFull` mapped). `writer.rs`'s tests check the order, a panicked job, the deadline and the inline mode. `engine/saves_tests.rs` saves through `ctx.saves()` in a headless engine and reads `SaveFinished`, drops an engine with a slow save still queued and finds it written, and runs a headless engine from a project folder (in a child process of the test binary, so no other test sees the change of folder) to check its saves stay in memory and the project's data folder is untouched.

`container.rs`'s tests round-trip a save, cut off and flip bytes in each region, refuse a foreign file and a newer format, and read the format-1 fixture. `versions.rs`'s tests upgrade through two steps, refuse a newer version, panic at registration on each mistake, and upgrade the fixture and JSON sections. `replace.rs`'s tests run a replace with a fault at every step: each `FaultOp` failing at each call, a torn write, and a cut-off wrapper that stops every operation from the kth on (as a process that died there leaves the store), and check a fresh `recover` finds the old or the new copy whole. `files.rs` runs the same order on a real folder, and two writers on one folder.
