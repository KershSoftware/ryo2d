# AGENTS.md - Saves Module

Game saves, from the slot name down to the bytes on disk. The save file carries a header, checksums and the game's data version, and older data upgrades through typed steps. The storage layer keeps files whole: a crash at any point leaves a whole, verified copy of either the old or the new bytes, and recovery finds it. The design is `docs/specs/2026-11-07-saves-and-settings-design.md` (where it and the code differ, the code is current).

| File | What |
|------|------|
| `mod.rs` | The exports: `SaveError`, `SaveHeader`, `SaveVersions` and `MemorySaves` are public; the rest is crate-internal |
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

`Storage` is what the saves use: `read`, `names`, `replace`, `recover`, `remove_all` and `set_aside`. Writes, removals and renames return a `Completion`: `Done` with the result, or `Pending` with a channel the result arrives on, for a backend that commits asynchronously (the browser's). `Completion::poll` checks without blocking and `wait` blocks; a pending completion whose sender is dropped is an error.

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
- `MemorySaves` is a `Clone` handle on shared state (`Arc<Mutex<..>>`): `put`, `get`, `names` and `remove` reach the files directly and are not counted as operations. Its faults are crate-internal:
  - `fail(op, nth)` fails the nth call of `op` and `fail_from(op, nth)` every call from the nth on, both 0-based and counted from when they are set, for `FaultOp::{Write, Rename, Remove, SyncDir}`;
  - `crash_write(nth, keep)` stores only the first `keep` bytes of the nth write, then fails;
  - `deny_rename(times)` fails the next renames with `PermissionDenied`;
  - `slow(per_op)` makes every operation wait (on native; the web cannot block);
  - `ops()` counts every `FileOps` call and `writes()` every write, failed ones included.

## Testing

`container.rs`'s tests round-trip a save, cut off and flip bytes in each region, refuse a foreign file and a newer format, and read the format-1 fixture. `versions.rs`'s tests upgrade through two steps, refuse a newer version, panic at registration on each mistake, and upgrade the fixture and JSON sections. `replace.rs`'s tests run a replace with a fault at every step: each `FaultOp` failing at each call, a torn write, and a cut-off wrapper that stops every operation from the kth on (as a process that died there leaves the store), and check a fresh `recover` finds the old or the new copy whole. `files.rs` runs the same order on a real folder, and two writers on one folder.
