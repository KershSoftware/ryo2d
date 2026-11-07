# AGENTS.md - Saves Module

Game saves, from the slot name down to the bytes on disk. This layer keeps files whole: a crash at any point leaves a whole, verified copy of either the old or the new bytes, and recovery finds it. The design is `docs/specs/2026-11-07-saves-and-settings-design.md` (where it and the code differ, the code is current).

| File | What |
|------|------|
| `mod.rs` | The exports: `SaveError` and `MemorySaves` are public; the rest is crate-internal |
| `error.rs` | `SaveError`, a `thiserror` enum whose causes are its `source()` |
| `slot.rs` | The slot rule (`check_new`), older names still reachable (`is_legacy`), and the file names: `file_name`, `backup_name`, `temp_name`, `is_temp_of` |
| `storage.rs` | The seam: `FileOps`, `Storage` (implemented for every `FileOps` backend), `Completion`, `Recovery`, `Origin` |
| `replace.rs` | The order of writing, recovering, removing and setting aside a file (`safe_replace`, `recover`, `remove_all`, `set_aside`) |
| `files.rs` | `FileStorage`, a native folder (not on the web) |
| `memory.rs` | `MemorySaves`, the in-memory store, with its faults for tests (`FaultOp`) |

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

`replace.rs`'s tests run a replace with a fault at every step: each `FaultOp` failing at each call, a torn write, and a cut-off wrapper that stops every operation from the kth on (as a process that died there leaves the store), and check a fresh `recover` finds the old or the new copy whole. `files.rs` runs the same order on a real folder, and two writers on one folder.
