# Saves and Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Crash-safe, versioned, off-thread game saves through `ctx.saves()`, and live, self-saving player settings through `ctx.settings()`, on native, headless and the web. Along the way, each project setting is declared once, and overlay drawing moves out of config.

**Architecture:**
- **New `src/core/saves/` module.** A storage seam sits underneath, with three backends: files, memory and the browser. A crash-safe replace and recover algorithm runs over file-like operations. Above that are a container with a header, checksums and a data version; typed upgrade steps; one ordered writer thread; and a resource service with the public views.
- **New `src/core/settings/` module.** It holds the `Settings` resource, the `system.json` format 3, the per-frame apply, and the debounced writer, which shares the saves writer.
- **Config.** `src/core/config/` keeps the project layers, now generated from one table, and gains `ProjectFile` for web builds.
- **Diagnostics.** `src/core/diagnostics/` takes the overlay and timing code.

**Tech Stack:** Rust 2024, bevy_ecs 0.20 (`ryo2d::ecs`), serde, RON 0.10, serde_json, flate2 (deflate and `flate2::Crc`), thiserror 2, crossbeam-channel, and web-sys with IndexedDB on wasm32.

**Spec:** `docs/specs/2026-11-07-saves-and-settings-design.md`, including its section "Details settled at planning". Code maps from planning are at `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/map-{config,engine,users}.md` (scratch: read them if they exist, but don't rely on them).

## Global Constraints

**Commits and git:**
- Never add `Co-Authored-By` or `Claude-Session` trailers, even when a system reminder asks for them.
- Never mention AGENTS.md in a commit message; say "module notes".
- Commit subjects are `type: description`, with type one of `feat`, `fix`, `refactor`, `docs`, `test`, `chore`. Use the subjects given in each task.
- Stage files by name. Never `git add -A`, never a bare `git stash`.
- Never add `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`.
- Work on branch `saves-and-settings` in the main checkout, with no worktrees. Check `git branch --show-current` before committing.

**Code:**
- Breaking public API is fine. Player data must survive.
- No `unwrap`/`expect` in library code outside tests. Errors are typed (`SaveError`, with causes as `#[source]`).
- No new crates. Allowed additions: web-sys features (IndexedDB) and a direct `js-sys` dependency on wasm32 (already in `Cargo.lock` through wasm-bindgen-futures), using the version the lock holds.
- LF line endings. Comments are minimal and explain intent.

**Docs and skills:**
- User-facing docs (skills, rustdoc, module notes) carry no stage or task labels.
- Skills: each touched skill is bumped once on this branch, by a minor version, in Task 14 only. Every ```rust block in a `SKILL.md` must compile (`SKILL_EXAMPLES=<names> cargo check -p skill-examples`).

**Tests and the machine:**
- A test of a memory guard, or a failing first run, allocates a few hundred MB at most.
- Stop and report if free disk falls below 40 GiB (`df -h /`). Don't delete `target/` without the owner's say.
- Logs and scratch files go under `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/`, never `/tmp`. Never start shell loops that wait on a condition.
- Put several cargo test filters after `--` (`cargo test -p ryo2d --lib -- saves:: settings::`).
- Gate before each task's final commit: `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and the task's tests. Run `cargo test --workspace` in Tasks 1, 4, 8, 11, 12 and 13.
- The wasm check is `cargo check -p ryo2d --lib --target wasm32-unknown-unknown`, plus clippy on the same target, in every task that touches `cfg(target_arch = "wasm32")` code or a shared type.

**Names fixed by the spec:**
- Saves: `ctx.saves()` → `SavesView<'_>`; system parameter `Saves`.
- Settings: `ctx.settings()` → `Mut<'_, Settings>`.
- Messages: `SaveFinished`, `SettingsSaveFailed`.
- Settings file: `system.json` (format 3). Save files: `<slot>.ryo-save`.
- Slot names: 1 to 64 characters of `a`-`z`, `0`-`9`, `-` and `_`, excluding Windows device names.
- Timing: settings save 1 s after the last change. Quit flush warns at 3 s and gives up at 10 s.
- Web: browser-store open timeout 2 s.
- Targets: `save` ≤ 20 µs on the main thread; idle settings ≤ 1 µs a frame.

## Review Focus

1. **A game that saves one slot every frame.** Writes stay bounded by coalescing, and memory doesn't grow. Task 4: `saving_one_slot_every_frame_writes_a_bounded_number_of_times`.
2. **A save folder that refuses writes** (read-only, unplugged, full). Each save reports an `Io` error naming the file, the last good save still loads, and the game keeps running. Settings warn once. Task 4: `a_store_that_refuses_writes_reports_each_save_and_keeps_the_last_good_one`; Task 9: `a_failed_settings_write_warns_once_and_sends_settings_save_failed`.
3. **The game launched twice, saving the same slot.** Temporary names are unique, so the folder ends with a verified save. Task 2: `two_writers_on_one_folder_leave_a_verified_save`.
4. **A slot loaded as another type.** It returns `SaveError::Deserialize { slot, .. }` and never panics. Task 4: `loading_a_slot_as_another_type_is_a_deserialize_error`.
5. **Data whose `Serialize` fails.** `SaveFinished` carries `Serialize`, and the previous save is untouched. Task 4: `a_save_whose_data_cannot_serialize_reports_it_and_keeps_the_previous_save`.

---

### Task 1: Overlay drawing and frame timings move to a diagnostics module

**Files:**
- Create: `src/core/diagnostics/mod.rs`, `timing.rs`, `overlays.rs`, `flags.rs`, `AGENTS.md`.
- Delete: `src/core/config/debug.rs`.
- Modify, following the importers listed in the code map:
  - `src/core/mod.rs`, `src/core/config/mod.rs`, `src/lib.rs` (the prelude's `FrameCost` path);
  - `src/core/ui/debug.rs`, `src/core/physics/debug.rs`;
  - `src/core/engine/{mod,frame,extract,tests}.rs`;
  - `src/core/console/{builtins,commands}.rs`, `src/core/testing/headless.rs`, `src/core/config/resolve.rs`;
  - `tests/integration/console.rs`, `src/core/physics/AGENTS.md` (the `config::debug::colors` reference).

**Interfaces:**
- **Produces** `ryo2d::core::diagnostics::{DebugConfig, DebugStats, SmoothedDebugStats, FrameTimings, SmoothedFrameTimings, FrameCost, OverlayMode, time_block, colors}`:
  - `timing.rs`: `FrameTimings`, `SmoothedFrameTimings`, `FrameCost`, `time_block`;
  - `overlays.rs`: `OverlayMode`, the stats types, `colors`, and the drawing code;
  - `flags.rs`: the runtime `DebugConfig`.
- `FrameCost` stays in the prelude.
- `render_fps_counter`, `render_frame_timing`, `render_stats`, `OverlayFlags`, `overlay_shapes`, `row_pitch`, `overlay_stack_top` and `overlay_label` become `pub(crate)`.
- `config` no longer re-exports any of these. `WorkspaceDebugConfig` stays until Task 5.

- [ ] **Step 1: Capture a baseline before any edit.** Run `./dev.sh demos:tour audio console ui text`, then copy `target/demo-tour/` to `<scratchpad>/tour-before-diagnostics/`. These demos draw the FPS overlay from their Ryo.toml.
- [ ] **Step 2: Move the code** into the four files as listed. Keep names, behaviour and pixel output. Update every importer.
- [ ] **Step 3: Write `src/core/diagnostics/AGENTS.md`** (module notes: what lives where and who draws the overlays).
- [ ] **Step 4: Run the gate and the whole suite.** Run `cargo test --workspace > <scratchpad>/t1.log 2>&1; tail -5 <scratchpad>/t1.log`. Expected: all pass, and clippy is clean.
- [ ] **Step 5: Compare the tour.** Run the same tour again and compare every PNG to the baseline. Expected: byte-identical, or zero differing pixels if PNG encoding varies. Report any difference with its page.
- [ ] **Step 6: Commit.** Stage the moved and modified files by name, with the message `refactor: overlay drawing and frame timings live in a diagnostics module`.

### Task 2: Crash-safe storage

**Files:**
- Create: `src/core/saves/mod.rs`, `error.rs`, `slot.rs`, `storage.rs`, `files.rs` (native folder), `memory.rs`, `replace.rs`, `AGENTS.md`.
- Modify: `src/core/mod.rs` (`pub mod saves`).

**Interfaces:**

`SaveError` (`saves/error.rs`) is public but not yet in the prelude (Task 4 does that). It derives thiserror, and every source is `#[source]`:
- `Io { path: String, source: std::io::Error }`
- `Corrupt { slot: String, reason: String }`
- `NewerVersion { found: u32, current: u32 }`
- `NotFound(String)`
- `InvalidSlot(String)`
- `StorageFull`
- `Unavailable`
- `Serialize(Box<dyn std::error::Error + Send + Sync>)`
- `Deserialize { slot: String, source: Box<dyn std::error::Error + Send + Sync> }`

Slot rules (`saves/slot.rs`):
- `pub(crate) fn check_new(slot: &str) -> Result<(), SaveError>`: the rule in Global Constraints. The Windows device names refused are `con`, `prn`, `aux`, `nul`, `com1`-`com9` and `lpt1`-`lpt9`.
- `pub(crate) fn is_legacy(slot: &str) -> bool`: true when `check_new` fails but today's rule accepts the name (non-empty, no `/` or `\`, no `..`, no leading `.`).
- Names:
  - `pub(crate) fn file_name(slot) -> String` is `"{slot}.ryo-save"`;
  - `pub(crate) fn backup_name(name) -> String` is `"{name}.bak"`;
  - `pub(crate) fn temp_name(name) -> String` is `"{name}.{pid}-{n}.tmp"`, where `n` is a process-wide atomic counter.

Storage (`saves/storage.rs`):
- `pub(crate) trait FileOps: Send + Sync`: `read`, `write_synced` (write and flush to disk), `rename` (atomic replace), `remove` (Ok if missing), `names`, `sync_dir`. All return `io::Result`.
- `pub(crate) enum Completion { Done(io::Result<()>), Pending(crossbeam_channel::Receiver<io::Result<()>>) }`
- `pub(crate) enum Recovery { Missing, Damaged, Found { bytes: Vec<u8>, from: Copy } }`, with `pub(crate) enum Copy { Current, Temp, Backup }`.
- `pub(crate) trait Storage: Send + Sync + 'static`:
  - `read(&self, name) -> io::Result<Option<Vec<u8>>>`
  - `names(&self) -> io::Result<Vec<String>>`
  - `replace(&self, name, bytes: &[u8], verify: &dyn Fn(&[u8]) -> Option<u64>) -> Completion`
  - `recover(&self, name, verify) -> io::Result<Recovery>`
  - `remove_all(&self, name) -> Completion` (the file, its backup and its temps)
  - `set_aside(&self, name, to) -> Completion` (rename, replacing `to`)
- `verify` returns `Some(recency)` for a whole, valid copy. `recover` uses the recency to pick the newest temp.

Algorithms (`saves/replace.rs`): `pub(crate) fn safe_replace(ops: &dyn FileOps, name, bytes, verify) -> io::Result<()>` and `pub(crate) fn recover(ops, name, verify) -> io::Result<Recovery>`. Every `FileOps` backend implements `Storage` through these.

Backends:
- `FileStorage::new(dir: PathBuf) -> io::Result<FileStorage>` (`saves/files.rs`) creates the directory.
- `MemorySaves` (`saves/memory.rs`) is public, `Clone`, with `Arc` state:
  - public: `new()`, `put(name, bytes)`, `get(name) -> Option<Vec<u8>>`, `names() -> Vec<String>`, `remove(name)`;
  - faults (`pub(crate)`): `fail(op: FaultOp, nth: usize)`, `crash_write(nth: usize, keep: usize)` (stores only `keep` bytes, then fails), `deny_rename(times: usize)` (fails with `PermissionDenied`), `slow(per_op: Duration)`, `ops() -> usize`, `writes() -> usize`;
  - `pub(crate) enum FaultOp { Write, Rename, Remove, SyncDir }`.

Algorithm (my call, from the spec's invariant). `safe_replace` runs:
1. `write_synced(temp)`.
2. If the current file reads and `verify` passes, `rename(current → backup)`. A current file that fails `verify` never touches the backup.
3. `rename(temp → current)`. On `PermissionDenied`, retry up to 10 times, 10 ms apart.
4. `sync_dir`. On failure, warn and still return Ok, since the data is in place.
5. Remove this name's other temps.

`recover` prefers a verified current file, then the verified temp with the highest recency, then a verified backup. If some copy exists but none verifies, it returns `Damaged`. If no copy exists, it returns `Missing`.

- [ ] **Step 1: Write the failing tests** in `replace.rs`, `slot.rs` and `files.rs`. Every test uses `MemorySaves` unless named otherwise.
  - `a_crash_at_any_step_leaves_the_old_or_the_new_save`. Start from a verified `b"old"` file. Count a clean replace's ops (`ops()`). For every k in that range, and for both `fail` and `crash_write` at k:
    - replace with `b"new"`;
    - on a fresh `recover`, assert `Found` with bytes equal to `b"old"` or `b"new"`.
    - In the tests, `verify` accepts only `b"old"` and `b"new"` whole.
  - `a_damaged_current_falls_back_to_the_backup`: `from == Origin::Backup`.
  - `a_damaged_current_never_becomes_the_backup`: corrupt current, good backup, then replace. The backup is unchanged.
  - `a_newer_complete_temp_wins_over_the_backup`: stop after step 2, then recover. `from == Origin::Temp`, bytes `b"new"`.
  - `both_copies_damaged_is_damaged`.
  - `a_held_file_is_renamed_after_a_short_retry`: `deny_rename(3)` still succeeds; `deny_rename(20)` returns `PermissionDenied`.
  - `leftover_temps_are_cleared_by_the_next_save`.
  - `two_writers_on_one_folder_leave_a_verified_save` (Review Focus 3): two `FileStorage`s on one `tempfile::tempdir()`, each on its own thread, each replacing the same name 100 times. The final `recover` is `Found`, and no `.tmp` names remain.
  - `a_replace_survives_on_a_real_folder` (`FileStorage`): round trip, and the backup exists after the second write.
  - `new_slot_names_follow_the_rule`:
    - accepts `"slot1"`, `"auto-save_2"`, and 64 × `'a'`;
    - rejects `""`, `"Slot1"`, `"a b"`, `"a.b"`, `"../x"`, 65 × `'a'`, `"con"` and `"lpt9"`.
  - `legacy_names_are_recognised`:
    - `is_legacy("My Save")` and `is_legacy("Hero.v2")` are true;
    - `is_legacy("../x")` and `is_legacy("slot1")` are false.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p ryo2d --lib -- saves::`. Expected: compile errors or FAIL, because the items aren't defined yet.
- [ ] **Step 3: Implement** the items above. `FileStorage::sync_dir` opens the directory and runs `sync_all` on Unix, and is a no-op on Windows.
- [ ] **Step 4: Run the tests again.** Expected: PASS. Then run the gate.
- [ ] **Step 5: Write `src/core/saves/AGENTS.md`**, covering the storage seam, the replace and recover order, and the temp and backup names.
- [ ] **Step 6: Commit** with the message `feat: saves are written so a crash keeps the old or the new copy`.

### Task 3: The save container and upgrade steps

**Files:**
- Create: `src/core/saves/container.rs`, `src/core/saves/versions.rs`, `tests/fixtures/saves/{legacy.ryo-save, system-format1.json, system-format2.json, README.md}`.
- Modify: `src/core/saves/mod.rs`, `src/core/saves/AGENTS.md`.

**Interfaces:**

Public in `saves/mod.rs`, re-exported to the prelude in Task 4:
- `SaveHeader { pub format: u32, pub version: u32, pub saved_at: u64, pub play_time: u64, pub name: Option<String>, pub labels: BTreeMap<String, String>, pub engine_version: String }`, with `serde` derives.

Container (`saves/container.rs`, `pub(crate)`), format 2. All integers are u32 little-endian:

| Bytes | Content |
|---|---|
| 0-4 | `b"RYO2"` |
| 4-8 | format = 2 |
| 8-12 | header length H |
| 12-16 | body length B |
| 16-20 | CRC-32 of the header |
| 20-24 | CRC-32 of the body |
| 24 onward | header (JSON of `SaveHeader`), then body (deflate of RON, level 6) |

Container functions:
- `encode(header: &SaveHeader, body_ron: &str) -> Result<Vec<u8>, SaveError>`
- `read_header(bytes: &[u8]) -> Result<SaveHeader, SaveError>`: checks the magic, format, lengths and the header CRC only.
- `verify(bytes: &[u8]) -> Option<u64>`: the whole file, returning `saved_at`.
- `decode(bytes) -> Result<(SaveHeader, Body), SaveError>`, with `pub(crate) struct Body { pub ron: String, pub legacy: bool }`.
- Errors:
  - foreign bytes → `Corrupt { reason: "not a Ryo2D save" }`;
  - format > 2 → `NewerVersion { found, current: 2 }`.

Format 1 (today's) reading:
- The file is magic, version 1, then deflated RON of `GameSaveData<T>`.
- `read_header` deserializes `{ data: serde::de::IgnoredAny, timestamp, name, play_time, metadata }` into a header with format 1, version 1 and labels taken from `metadata`.
- `decode` returns the whole RON with `legacy: true`.

Versions (`saves/versions.rs`):
- `pub struct SaveVersions<T>` with:
  - `pub fn new(current: u32) -> Self`
  - `pub fn step<A: DeserializeOwned + 'static, B: Serialize + 'static>(self, from: u32, convert: impl Fn(A) -> B + Send + Sync + 'static) -> Self`
- Each step keeps both a RON conversion (string to string) and a JSON one (`serde_json::Value` to `Value`).
- `pub(crate) struct VersionRegistry`, keyed by `TypeId`:
  - `register::<T>(SaveVersions<T>)` panics if `current == 0`, on a missing step (`"SaveVersions<{type}>: missing step from version {n}"`) or on a duplicate (`"... two steps from version {n}"`);
  - `current::<T>() -> u32` returns 1 when T is unregistered;
  - `decode_ron::<T>(version, &Body) -> Result<(T, Option<u32>), SaveError>`. A `legacy` body is unwrapped to its `data` field before step 1, by deserializing the version-1 type inside a typed `{ data: A, .. }` wrapper. `version > current` gives `NewerVersion`;
  - `decode_json::<T>(key, version, Value) -> Result<(T, Option<u32>), SaveError>`, for settings sections.

- [ ] **Step 1: Make the fixtures before any change to `config/save.rs`.**
  - In a temporary `#[test] #[ignore]` inside `config/save.rs` (removed before commit), use today's `SaveManager`:
    - write `GameSaveData::new(LegacyFixture { level: 3, name: "Ada".into(), items: vec!["key".into(), "lamp".into()] })`;
    - with `.with_name("Before the update")`, `.with_play_time(754)`, `.with_metadata("chapter", "2")` and `timestamp = 1_700_000_000`;
    - to `tests/fixtures/saves/legacy.ryo-save`.
  - Use today's `save_system` to write `system-format2.json` from a `SystemState`: `master_volume` 0.8, every other value at the engine defaults, `env` `{"difficulty": "hard"}`.
  - Hand-write `system-format1.json`, with no `format_version`: `"master_volume": 0.25`, `"music_volume": 1.0`, `"resolution": "1600x900"`, `"display_mode": "windowed"`, `"vsync": true`, `"env": {"difficulty": "hard"}`.
  - The README names the commit the fixtures came from, and the `LegacyFixture` struct.
  - Commit the fixtures and README alone: `test: saves and settings files from before the new formats are kept as fixtures`.
- [ ] **Step 2: Write the failing tests.**
  - `a_save_round_trips_with_its_header`.
  - `the_header_reads_without_the_body`: truncate the body; `read_header` is Ok and `verify` is None.
  - `a_flipped_byte_in_the_header_or_the_body_fails_verification`: one case per region.
  - `a_file_of_another_kind_is_not_a_save`: PNG magic gives `Corrupt`.
  - `a_newer_container_format_is_refused`: format 3 gives `NewerVersion { found: 3, current: 2 }`.
  - `the_legacy_fixture_reads_with_its_header`: format 1, version 1, name `"Before the update"`, `play_time` 754, labels `{"chapter": "2"}`; the decoded body gives the fixture's values.
  - `a_version_one_save_upgrades_through_two_steps`:
    - V1 `{ hp: u32 }` → V2 `{ hp: u32, mp: u32 }` (mp = 0) → V3 `{ health: u32, mana: u32 }`;
    - V1 hp 7 gives V3 `{ health: 7, mana: 0 }` with `upgraded_from == Some(1)`.
  - `a_newer_save_is_refused`: version 4 with current 3 gives `NewerVersion { found: 4, current: 3 }`.
  - `a_gap_in_the_steps_panics_at_registration`: `#[should_panic(expected = "missing step from version 2")]`.
  - `an_unregistered_type_is_version_one`.
  - `the_legacy_wrapper_upgrades_from_version_one`: the fixture as V1 of a two-step chain.
  - `json_sections_upgrade_with_the_same_steps`.
- [ ] **Step 3: Run them to see them fail.** Run `cargo test -p ryo2d --lib -- saves::container saves::versions`. Expected: FAIL or compile errors.
- [ ] **Step 4: Implement** `container.rs` and `versions.rs`, using `flate2::Crc` for the CRC-32.
- [ ] **Step 5: Run the tests again.** Expected: PASS. Then run the gate, and extend `saves/AGENTS.md`.
- [ ] **Step 6: Commit** with the message `feat: saves carry a header, checksums and the game's data version, with typed upgrade steps`.

### Task 4: `ctx.saves()`, the writer and the engine

**Files:**
- Create: `src/core/saves/writer.rs`, `service.rs`, `view.rs`.
- Modify:
  - `src/core/saves/mod.rs`, `src/core/messages/mod.rs`, `src/core/scene/context.rs`;
  - `src/core/engine/{mod.rs, window.rs, frame.rs}`;
  - `src/core/testing/{mod.rs, headless.rs}`, `src/lib.rs`;
  - `src/core/error.rs` and `src/core/config/{save.rs, workspace.rs}` (the old error rename);
  - `src/core/saves/AGENTS.md`.

**Interfaces:**

Public, and in the prelude:
- `SaveInfo { pub name: Option<String>, pub play_time: u64, pub labels: BTreeMap<String, String> }`. It derives `Default` and `Clone`, with `new()`, `name(s)`, `play_time(secs)` and `label(k, v)` builders.
- `Loaded<T> { pub data: T, pub header: SaveHeader, pub from_backup: bool, pub upgraded_from: Option<u32> }`
- `SaveSlot { pub slot: String, pub header: SaveHeader, pub from_backup: bool }`
- `#[derive(Message, Clone, Debug)] SaveFinished { pub slot: String, pub op: SaveOp, pub result: Result<(), Arc<SaveError>> }`, with `pub enum SaveOp { Save, Delete }`.
- `SaveHeader`, `SaveError` (the new one) and `SaveVersions`.

`SavesView<'w>` (from `ctx.saves()`) and `Saves<'w>` (a `SystemParam` over `Res<SaveService>`) share these `&self` methods:
- `save<T: Serialize + Send + 'static>(&self, slot: &str, data: T) -> Result<(), SaveError>`
- `save_with<T>(&self, slot, data: T, info: SaveInfo) -> Result<(), SaveError>`
- `load<T: DeserializeOwned + 'static>(&self, slot) -> Result<Loaded<T>, SaveError>`
- `info(&self, slot) -> Result<SaveHeader, SaveError>`
- `list(&self) -> Result<Vec<SaveSlot>, SaveError>`, sorted by slot
- `exists(&self, slot) -> bool`
- `delete(&self, slot) -> Result<(), SaveError>`
- `is_writing(&self, slot) -> bool`

The service is internally synchronized.

Testing and the builder:
- `ryo2d::testing::MemorySaves` re-exports `saves::MemorySaves`.
- `HeadlessEngine::saves(&self) -> MemorySaves`.
- `EngineBuilder::save_versions::<T>(self, SaveVersions<T>) -> Self` panics at the call on a gap.
- `EngineBuilder::memory_saves(self, MemorySaves) -> Self`.

Crate-internal:
- `SaveService` resource:
  - `set_storage(Arc<dyn Storage>)`, used by Task 10;
  - `write_named(name: &str, bytes: Vec<u8>, verify: fn(&[u8]) -> Option<u64>) -> u64` (a job id), for settings;
  - `flush(warn_after: Duration, give_up: Duration) -> Vec<String>` (unfinished names);
  - `drain() -> Vec<Finished>`.
- The old `crate::core::error::SaveError` is renamed `SaveManagerError` (with `EngineError::Save(SaveManagerError)`) until Task 12.

Behaviour:
- **`save`:**
  - The main thread checks the slot with `check_new` (else `InvalidSlot`), returns `Unavailable` when there is no storage, takes the type's version from the registry, and builds the header (`saved_at` from `web_time::SystemTime`, `engine_version` from `env!("CARGO_PKG_VERSION")`).
  - It queues a job on the writer and records a pending entry for the slot, with a generation number.
  - The job:
    1. If a newer generation of the slot is queued, it skips its work and reports Ok.
    2. Otherwise it encodes, publishing the encoded bytes (or the error) to the pending entry.
    3. Then it runs `storage.replace(file_name, bytes, container::verify)`.
- **`load`:**
  - A pending write: wait for its encoded bytes (not the disk), then decode them.
  - A pending delete: `NotFound`.
  - Otherwise `recover`:
    - `Missing` is `NotFound`; `Damaged` is `Corrupt { slot }`;
    - an `Origin::Backup` copy sets `from_backup` and warns once per slot;
    - then `decode` and `decode_ron::<T>`.
  - Names: a name `check_new` refuses is accepted only on native, when `is_legacy` holds and that file exists.
- **Writer:**
  - `writer.rs` follows `src/core/tilemap/jobs.rs` `Worker`: one thread named `"ryo2d-saves"`, FIFO, run inline where no thread starts (web).
  - `flush` drains the queue, warns at `warn_after` and gives up at `give_up`, logging the unfinished names.
- **Results:** `drain_save_results` runs in `Phase::PreUpdate` and writes `SaveFinished`, clearing pending entries whose generation matches.
- **Storage:**
  - Native, not headless: `FileStorage` at `Workspace::save_data_dir()`.
  - Headless: the builder's `MemorySaves`, else a new one.
  - No workspace (web until Task 10): `Unavailable`.
- **Flush:** at quit, `SaveService::flush(3 s, 10 s)` runs from `ApplicationHandler::exiting` (`engine/window.rs`, before the capture flush), at the end of `run_offscreen_frames`, and in a new `Drop for Engine`. It is idempotent.

- [ ] **Step 1: Write the failing service tests** (`saves/service.rs`; `SaveService` over `MemorySaves`, no engine):
  - `a_load_while_a_write_is_queued_returns_the_queued_data`: `slow(50ms)`.
  - `two_saves_to_one_slot_write_the_later_and_report_both`: one store write, two Ok results.
  - `a_delete_after_a_queued_save_leaves_no_slot`.
  - `pending_writes_finish_before_flush_returns`.
  - `flush_warns_then_gives_up`: `flush(30ms, 100ms)` on `slow(1s)` returns the unfinished name.
  - `saving_one_slot_every_frame_writes_a_bounded_number_of_times` (Review Focus 1): 1,000 saves on `slow(5ms)`, then flush. `writes() <= 10`, and the stored data is the last save's.
  - `a_store_that_refuses_writes_reports_each_save_and_keeps_the_last_good_one` (Review Focus 2): `fail(Write, n)` for every write after the first good save. Each result is `Err(Io { path })` with the file name in `path`, and `load` returns the first save.
  - `loading_a_slot_as_another_type_is_a_deserialize_error` (Review Focus 4): save `A { x: u32 }`, load `B { name: String }`, get `Deserialize { slot: "a" }`.
  - `a_save_whose_data_cannot_serialize_reports_it_and_keeps_the_previous_save` (Review Focus 5): a type whose `Serialize` returns `Err(S::Error::custom("no"))` gives `Err(Serialize(_))`, and `load` returns the earlier save.
  - `a_slot_damaged_on_disk_loads_from_its_backup_with_a_warning`.
  - `old_slot_names_stay_reachable_on_native`: `FileStorage` with `"My Save.ryo-save"` (the legacy fixture's bytes). `load` is Ok and `save("My Save", ..)` is `InvalidSlot`.
  - `the_legacy_fixture_loads_through_the_service`: as `LegacyFixture`.
- [ ] **Step 2: Write the failing engine tests** (`HeadlessEngine::new_or_skip`):
  - `a_scene_saves_and_loads_through_ctx_saves_and_reads_save_finished`.
  - `dropping_the_engine_finishes_pending_saves`: a headless engine on `MemorySaves::slow(100ms)` saves, then is dropped. The store holds the slot afterwards.
  - `a_headless_engine_keeps_saves_in_memory`: after a save, `HeadlessEngine::saves().names()` holds `"slot1.ryo-save"`, and the workspace's data folder is unchanged. Run from a `tempfile` cwd holding a Ryo.toml.
- [ ] **Step 3: Run them to see them fail.** Run `cargo test -p ryo2d --lib -- saves::service engine::`. Expected: FAIL or compile errors.
- [ ] **Step 4: Implement** the writer, service, views and builder methods; the message registration (after `ChunkEvent`); the `ctx.saves()` accessor (`SavesView::new(self.world)`, as `audio()`); the flush hooks; and the prelude exports.
- [ ] **Step 5: Run the tests, the gate and `cargo test --workspace`.** Expected: PASS.
- [ ] **Step 6: Commit** with the message `feat: ctx.saves() saves on a background thread, loads what it queued, and finishes writes at quit`.

### Task 5: Each project setting declared once

**Files:**
- Create: `src/core/config/project.rs` (the crate's `project_settings!` interpreter, `TableValue`, the generated `ProjectLayer`) and `src/core/config/project_table.rs` (only the table).
- Modify:
  - `src/core/config/{resolve.rs, workspace.rs, mod.rs, AGENTS.md}`, `src/build.rs`;
  - `src/cli/commands/build.rs` (`embedded_config_env` from the table; its tests), `src/cli/commands/init.rs` (template comments);
  - `src/core/engine/mod.rs` (one `DebugConfig`);
  - Ryo.toml files: the stray sections in `controller`, `dpi-scaling`, `validated-animator`, `ui`, `networking`, `pathfinding`, `procgen` and `profile-and-log`; `cursor`'s `[system] width`/`height` become `resolution = "800x600"`; the stale `system.json` comments in the five games and in `y-sorting`, `shadows` and `lighting`.

**Interfaces:**

The table file:
- `project_table.rs` is one macro invocation, with one line per setting:
  ```
  project_settings! {
    resolution: Resolution = system.resolution, embed "RYO_EMBED_RESOLUTION", sample "1600x900";
    master_volume: f32 = system.master_volume, embed "RYO_EMBED_MASTER_VOLUME", sample 0.5;
    ...
  }
  ```
- It has entries for every current `ProjectLayer` field, keeping today's embed names (`dev_console` = `system.console` / `RYO_EMBED_CONSOLE`; `background_fps` = `system.target_background_fps` / `RYO_EMBED_BACKGROUND_FPS`). `icon` has no embed key.
- A second block, `workspace_keys { project.name, project.title?, project.version, project.description, paths.entry_point, paths.save_data, paths.assets, release.logo, release.portable embed "RYO_EMBED_PORTABLE", release.save_folder embed "RYO_EMBED_SAVE_FOLDER" }`, plus `env embed "RYO_EMBED_ENV"`. Exact syntax is the implementer's, but both interpreters must parse it.

What the crate's interpreter generates:
- `pub(crate) struct ProjectLayer`, with `Option` fields;
- `ProjectLayer::from_toml(&toml::Table, warn: &mut dyn FnMut(String))`;
- `ProjectLayer::from_embedded(lookup)`, keeping today's rule: None unless `RYO_EMBED_RESOLUTION` is present;
- `ProjectLayer::to_embedded_env(&self) -> Vec<(&'static str, String)>`;
- `pub(crate) const EMBED_KEYS: &[&str]`;
- `pub(crate) const KNOWN_KEYS: &[(&str, &str)]`;
- `#[cfg(test)] fn sample_layers() -> Vec<(&'static str, ProjectLayer)>`, one per entry.

`TableValue` is implemented for `bool`, `f32`, `u32`, `String`, `PathBuf`, `DisplayMode` and the resolution type.

`src/build.rs` defines its own `macro_rules! project_settings` that keeps only the embed keys, then does `include!("core/config/project_table.rs")`.

Hand-written code that stays:
- `ResolvedSettings`, `BuilderOverrides` and `resolve_settings`;
- the CLI's special values: `RYO_EMBED_RESOLUTION` always set (fallback `"1280x720"`); `RYO_EMBED_DEV_WINDOW_KEYS` from an explicit value only, else `"false"`; `SAVE_FOLDER` computed as today; `ENV` as JSON.

Workspace changes:
- The serde section structs `SystemSection`, `DebugSection`, `ProjectSection` and the rest are replaced by reading a `toml::Table` through the table.
- `WorkspaceConfig.debug` becomes `diagnostics::DebugConfig`. Delete `workspace::DebugConfig` and the `WorkspaceDebugConfig` alias.
- `WorkspaceConfig.system: SystemDefaults` stays until Task 12, filled from the layer.
- `pub(crate) fn Workspace::from_toml_str(text: &str, root: PathBuf) -> Result<Workspace, EngineError>` is the one reader of Ryo.toml text. Disk loading calls it too.

The misspelt-key warning:
- For a key in a known section (`project`, `paths`, `system`, `debug`, `release`, `tilemap`) that isn't in `KNOWN_KEYS`, warn once: ``Ryo.toml: unknown key `{key}` in [{section}]``, adding `` (did you mean `{nearest}`?)`` when the nearest known key in that section is within Levenshtein distance 3.
- `[env]` and unknown sections never warn.

- [ ] **Step 1: Write the failing tests.**
  - `every_table_entry_reaches_the_resolved_settings`: for every entry in `sample_layers()`, the resolved settings differ from those of the default layer.
  - `every_embed_key_round_trips`: a layer with every sample, through `to_embedded_env` then `from_embedded`, comes back equal.
  - `a_ryo_toml_with_every_key_sets_every_field`: TOML text built from the samples gives every field `Some`.
  - `a_misspelt_key_warns_with_the_nearest_known_key`: `[system]\nvsyn = true` warns, and the warning contains `` `vsync` ``.
  - `keys_outside_known_sections_do_not_warn`: `[env] foo = 1` and `[custom] x = 2`.
  - In `src/cli/commands/build.rs`, keep `embedded_env_covers_every_key_the_engine_reads`, comparing against `EMBED_KEYS`. Replace `build_script_tracks_exactly_the_embedded_keys` with `the_build_script_reads_the_project_table`, which asserts that `include_str!("../../build.rs")` contains `include!("core/config/project_table.rs")`.
  - Adapt the remaining resolve, workspace and CLI tests to the generated layer.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p ryo2d --lib -- config:: cli::`. Expected: FAIL or compile errors.
- [ ] **Step 3: Implement** the table, both interpreters, the workspace reader, the single `DebugConfig` and the warning. Clean up the Ryo.toml files and the comment text. The new comment line is: `# Players' changes to these are kept in system.json (only what differs from here)`.
- [ ] **Step 4: Run the tests and the gate.** Then run `cargo run -p cursor-demo -- --help 2>&1 | head` (or any demo start) and check no Ryo.toml warning is printed. Expected: PASS and silent.
- [ ] **Step 5: Update `src/core/config/AGENTS.md`**: the table, the two interpreters and adding a setting in one place.
- [ ] **Step 6: Commit** with the message `refactor: each project setting is declared once, and a misspelt Ryo.toml key warns`.

### Task 6: Web builds read the game's Ryo.toml

**Files:**
- Create: `src/core/config/project_file.rs`, `tests/fixtures/project/Ryo.toml`.
- Modify:
  - `crates/ryo2d-derive/src/lib.rs` (`#[proc_macro] project`), `src/lib.rs` (`pub use ryo2d_derive::project;`);
  - `src/core/config/{mod.rs, workspace.rs, save_location.rs}`, `src/core/engine/mod.rs`;
  - `tests/integration.rs` and a new `tests/integration/project_file.rs`.

**Interfaces:**
- `ryo2d::project!()` and `ryo2d::project!("relative/path")`:
  - expand at compile time in the calling crate, relative to that crate's `CARGO_MANIFEST_DIR`;
  - the default tries `Ryo.toml`, then `ryo.toml`;
  - a found file gives `::ryo2d::core::config::ProjectFile::from_toml(include_str!("<absolute path>"))`, and a missing one gives `ProjectFile::none()`.
- `pub struct ProjectFile` with `from_toml(&'static str)`, `none()` and `is_none()`.
- `EngineBuilder::project(self, ProjectFile) -> Self`.
- Project-layer sources in `resolve_in`, first found wins:
  1. embedded `RYO_EMBED_*` (native release builds);
  2. `Workspace::discover()` from disk (native dev);
  3. `ProjectFile` through `Workspace::from_toml_str` (web, or native with no Ryo.toml found);
  4. defaults.
- `pub(crate) fn store_name(release_save_folder: Option<&str>, project_name: Option<&str>, title: &str) -> String`:
  - `[release] save_folder` if `valid_save_folder` accepts it;
  - else `[project] name`, sanitized as `release_save_folder` sanitizes it;
  - else the title, warning once: "web saves are named after the window title; set [project] name so other games on the same site don't share them".
  - The engine keeps the result for Task 10.

- [ ] **Step 1: Write the failing tests.**
  - The fixture `Ryo.toml` sets `[project] name = "Fixture Game"`, `[system]` resolution `"1600x900"`, `vsync = false`, `master_volume = 0.5`, `display_mode = "borderless_fullscreen"`, `[debug] show_colliders = true`, `[release] save_folder = "Studio/Fixture"` and `[env] difficulty = "hard"`.
  - `a_web_style_resolve_matches_the_release_resolve_of_the_same_ryo_toml`: resolving via `ProjectFile`, with no workspace and no embedded values, equals resolving via an embedded lookup built from `embedded_config_env` of the same file. Compare `ResolvedSettings` with a test-only `PartialEq` or `Debug`.
  - `the_project_macro_reads_a_ryo_toml_beside_the_manifest`: `ryo2d::project!("tests/fixtures/project/Ryo.toml")` has the same text as `fs::read_to_string`.
  - `the_project_macro_is_none_without_a_ryo_toml`: `ryo2d::project!()` at the repo root, which has no Ryo.toml.
  - `the_store_name_prefers_save_folder_then_name_then_title`: also checks the title fallback warns.
  - `a_dev_build_prefers_the_ryo_toml_on_disk`.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test --test integration -- project_file` and `cargo test -p ryo2d --lib -- config::`. Expected: FAIL.
- [ ] **Step 3: Implement** the macro, `ProjectFile`, the builder method, the source order and `store_name`. Run the wasm check and clippy too.
- [ ] **Step 4: Run the tests and the gate.** Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat: web builds read the game's Ryo.toml through ryo2d::project!()`.

### Task 7: The `Settings` resource and the settings file

**Files:**
- Create: `src/core/settings/{mod.rs, resource.rs, sections.rs, file.rs, AGENTS.md}`.
- Modify: `src/core/mod.rs`, `src/lib.rs` (prelude `Settings`).

**Interfaces:**

`#[derive(Resource)] pub struct Settings`, with private fields:
- Engine values:
  - `volume(&self, VolumeKind) -> f32` and `set_volume(&mut self, VolumeKind, f32)`. Setters clamp to 0..=1, and NaN means the default (`VolumeKind::sanitize`).
  - `window_size(&self) -> (u32, u32)` and `set_window_size(&mut self, u32, u32)`, in logical pixels.
  - `display_mode`/`set_display_mode`, `vsync`/`set_vsync`, `show_fps`/`set_show_fps`.
- Defaults and filter:
  - `reset(&mut self, UserPreference)`; `reset(Bindings)` does nothing and is documented as pointing to `ctx.actions().reset_defaults()`.
  - `reset_all(&mut self)`.
  - `is_kept(&self, UserPreference) -> bool`.
- Game sections:
  - `game<T: 'static>(&self) -> &T` and `game_mut<T: 'static>(&mut self) -> &mut T`;
  - both panic for an unregistered T: `"Settings::game::<{type}>: register it with EngineBuilder::game_settings"`.

Crate-internal:
- `EngineValues { volumes: [f32; 6], window_size: (u32, u32), display_mode, vsync: bool, show_fps: bool }`.
- `Settings::new(defaults: EngineValues, kept: UserPreferenceFilter, sections: SectionRegistry) -> Settings`.
- `Settings::apply_file(&mut self, &SettingsFile, warnings: &mut Vec<String>)`: kept values and sections only.
- `Settings::to_file(&self, base: &SettingsFile) -> SettingsFile`: only kept values that differ from the defaults; sections as stored below; `base`'s unknown keys and sections carried over.
- `SectionRegistry::register::<T: Default + Serialize + DeserializeOwned + Send + Sync + 'static>(key: &str, versions: Option<SaveVersions<T>>)`.

The file (`settings/file.rs`):
- `SettingsFile { format: u32, values: FileValues, bindings: Option<BTreeMap<String, Vec<String>>>, sections: BTreeMap<String, RawSection>, unknown: serde_json::Map<String, Value> }`, with `RawSection { version: u32, value: Value }`.
- `pub(crate) enum Parsed { Read { file, warnings: Vec<String> }, Newer { file, warnings }, Damaged(String) }`.
- `parse(bytes: &[u8], project_env: &serde_json::Map<String, Value>) -> Parsed`.
- `render(&SettingsFile) -> Vec<u8>`: pretty JSON with a trailing LF, keys in this order: `format_version` (3), the engine keys, `bindings`, `game`, the unknown keys.

File keys:
- `master_volume` … `ambience_volume` (rounded to two decimals);
- `resolution` (`"WxH"`), `display_mode` (snake_case), `vsync`, `show_fps`;
- `bindings`;
- `game`: `{ "<key>": { "version": N, "value": ... } }`. For a section whose JSON is an object, `value` holds only the top-level fields that differ from `T::default()`, and a load merges them over the default's object. Other sections store the whole value.

Conversions:
- Format 1, meaning no `format_version` or a value below 2: volumes become `√v`, rounded to two decimals (today's `RawSystemFile::upgrade`).
- Formats 1 and 2: the legacy `display` object and the `width`/`height` forms are read as today. `env` values that differ from `project_env` become `sections["env"] = { version: 1, value: {…} }`.
- Format 3: as above.
- Format above 3: `Newer`.
- A value of the wrong type, or out of range (a volume that isn't finite or lies outside 0..=1), falls back to the default for that key, with the warning ``system.json: `{key}` has a bad value ({detail}); using the default``.
- A file that isn't a JSON object is `Damaged`.

- [ ] **Step 1: Write the failing tests:**
  - `a_setting_changed_from_its_default_is_the_only_engine_key_written`
  - `a_setting_set_back_to_its_default_leaves_the_file`
  - `a_default_changed_in_the_project_reaches_a_setting_the_player_never_set`
  - `volumes_are_clamped_and_nan_means_the_default`
  - `the_format_1_fixture_converts_its_volumes_and_keeps_env_changes`: master 0.25 → 0.5. With `project_env` `{"difficulty": "normal"}`, `game.env.value == {"difficulty": "hard"}`.
  - `the_format_2_fixture_keeps_only_values_that_differ_from_the_defaults`: the rendered JSON holds `master_volume: 0.8`, the env section and nothing else.
  - `unknown_keys_survive_a_write`
  - `a_newer_format_is_read_and_reported_newer`
  - `a_file_that_is_not_json_is_damaged`
  - `a_bad_value_falls_back_with_a_warning_and_the_rest_applies`
  - `a_game_section_round_trips_only_changed_fields`
  - `a_game_section_upgrades_with_its_steps`
  - `an_unregistered_section_panics_with_a_hint`: `#[should_panic(expected = "register it with EngineBuilder::game_settings")]`
  - `reset_all_returns_every_setting_to_the_game_s_defaults`
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p ryo2d --lib -- settings::`. Expected: FAIL.
- [ ] **Step 3: Implement** the items. Port the format 1 math and the legacy readers from `config/save.rs`, and leave the originals in place until Task 12.
- [ ] **Step 4: Run the tests and the gate.** Expected: PASS. Write `src/core/settings/AGENTS.md`.
- [ ] **Step 5: Commit** with the message `feat: Settings holds the player's settings, and its file keeps only what differs from the defaults`.

### Task 8: Settings apply the moment they change

**Files:**
- Create: `src/core/settings/apply.rs`.
- Modify:
  - `src/core/engine/{mod.rs, frame.rs, window.rs, tests.rs}`;
  - `src/core/graphics/renderer.rs`, `src/core/config/{resolve.rs, save.rs}`;
  - `src/core/settings/{mod.rs, AGENTS.md}`.

**Interfaces:**

Builder:
- `EngineBuilder::game_settings::<T>(self, key: &str) -> Self`
- `EngineBuilder::game_settings_versioned::<T>(self, key: &str, versions: SaveVersions<T>) -> Self`

Change detection (`settings/apply.rs`):
- `pub(crate) struct SettingsChanges { volumes: Vec<(VolumeKind, f32)>, display_mode: Option<DisplayMode>, window_size: Option<(u32, u32)>, vsync: Option<bool>, show_fps: Option<bool> }`
- `pub(crate) fn diff(applied: &EngineValues, now: &EngineValues) -> SettingsChanges`

Renderer: `WgpuRenderer::set_vsync(&mut self, vsync: bool)`, next to `resize`:
- reconfigures the surface's present mode and updates `presentation_blocks`;
- an offscreen renderer and the web do nothing;
- `pub(crate) fn present_mode_for(vsync: bool, supported: &[wgpu::PresentMode]) -> wgpu::PresentMode`: true gives `Fifo`; false gives `Mailbox` if supported, else `Immediate`, else `Fifo`. `new` uses it too.

Start-up (`resolve_in`):
- **The defaults:** `resolve_settings` gives the resolved defaults (layers 1-3); the preferences parameter and `StoredPreferences` are removed.
- **Persistence:** a build keeps settings when it isn't headless and either the kept filter is non-empty or sections are registered.
- **Reading the file:** a build that keeps settings reads `system.json` through the save storage. A headless build reads its `MemorySaves` (ruling: never disk).
- **Damaged files:** a `Damaged` file is set aside to `system.json.damaged`. The backup is used if it parses, else the defaults (ruling). The engine warns.
- **Engine config:** the window size, display mode, vsync and volumes come from `Settings`.
- **Bindings:** they go from the file to `saved_bindings`.
- **Removed:** `load_user_preferences` and `SaveManager::load_preferences_or_seed`, with their tests. `bindings_saver` stays until Task 9.

Each frame:
- `Engine::apply_settings(&mut self)` runs right after the fixed-step loop (`frame.rs`, where `save_changed_bindings` is called), before the Quit return.
- Its idle cost is one change check on the `Settings` resource. When something changed, it diffs against `AppliedSettings`:
  - volumes go to `AudioEngine::set_volume_setting`;
  - display mode and window size go into the `WindowRequest` fields, converting logical to physical with the scale factor, with window size applied only while windowed;
  - vsync sets `Engine.pending_vsync`, which `about_to_wait` applies through `set_vsync`, updating `Engine.vsync`;
  - FPS sets the overlay flags, as the F-key path does.

OS changes:
- In the `Resized` arm, while windowed and not minimized, the logical inner size goes into `Settings` and `AppliedSettings` together (no echo).
- `window.fullscreen()` maps to a `DisplayMode`. If it differs, the arm calls `Engine::set_display_mode`, which now updates `Settings` and `AppliedSettings`. F5 goes through it as well.

- [ ] **Step 1: Write the failing tests:**
  - `a_volume_change_reaches_the_mixer_that_frame` (headless): a scene's `update` sets Music 0.3, and after one step `AudioEngine::volume_setting(Music) == 0.3`.
  - `settings_changes_become_window_and_renderer_requests`: `diff` for each field.
  - `the_vsync_setting_picks_the_present_mode`
  - `an_os_fullscreen_change_updates_settings_without_echo`
  - `the_player_file_overrides_the_project_for_kept_settings_only`: `MemorySaves` with `system.json`, ported from `saved_preference_beats_builder` and `preferences_outside_the_filter_are_ignored`.
  - `a_damaged_settings_file_is_set_aside_and_the_backup_used`
  - `a_headless_engine_reads_settings_from_its_memory_store_and_never_from_disk`
  - `a_game_section_registered_on_the_builder_is_readable_in_the_first_scene`
  - Replace the seeding tests in `engine/tests.rs` with `first_run_writes_no_settings_file_until_a_setting_changes`.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p ryo2d --lib -- settings:: engine::`. Expected: FAIL.
- [ ] **Step 3: Implement.** Run the wasm check and clippy.
- [ ] **Step 4: Run the tests, the gate and `cargo test --workspace`.** Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat: settings apply the moment they change, from ctx.settings()`.

### Task 9: Settings and bindings save themselves

**Files:**
- Create: `src/core/settings/persist.rs`.
- Modify: `src/core/engine/{mod.rs, frame.rs, window.rs}`, `src/core/messages/mod.rs`, `src/lib.rs`, `src/core/settings/AGENTS.md`.

**Interfaces:**
- `#[derive(Message, Clone, Debug)] pub struct SettingsSaveFailed { pub error: Arc<SaveError> }`, in the prelude and registered.
- `pub(crate) const DEBOUNCE: Duration = Duration::from_secs(1);`
- `pub(crate) struct SettingsWriter`:
  - `note_change(now: Instant)`
  - `set_bindings(map)`
  - `tick(now, &Settings, &SaveService) -> bool` (wrote)
  - `flush_now(&Settings, &SaveService)`
  - `read_only: bool` (a newer file), plus a warned-once flag.
- `now` is `web_time::Instant`, passed in so tests use made-up instants.
- Writes use `SaveService::write_named("system.json", render(..), json_verify)` on the shared ordered writer.
- A failed result warns once a run and sends `SettingsSaveFailed`.

Hooks:
- After `apply_settings`, `actions.take_dirty()` leads to `set_bindings(actions.saved_map())`. This replaces `save_changed_bindings`, and deletes `bindings_saver` and `bindings_save_warned`.
- A change to a kept value or a section calls `note_change`, then `tick`.
- `flush_now` runs:
  - in `ApplicationHandler::exiting`, before the saves flush;
  - in `WindowEvent::Occluded(true)`;
  - in a new `ApplicationHandler::suspended`;
  - at the end of `run_offscreen_frames`;
  - in `Drop for Engine`.

- [ ] **Step 1: Write the failing tests:**
  - `a_slider_drag_over_sixty_frames_writes_the_file_once`: 60 changes 16 ms apart, then `tick` at +1 s. `MemorySaves::writes()` rises by exactly one.
  - `bindings_land_in_the_same_file_as_the_settings`
  - `a_binding_set_back_to_its_default_leaves_the_file`: ported from the bindings tests.
  - `quitting_writes_a_pending_change_at_once`
  - `a_failed_settings_write_warns_once_and_sends_settings_save_failed` (Review Focus 2)
  - `a_newer_settings_file_is_never_overwritten`
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p ryo2d --lib -- settings::persist engine::`. Expected: FAIL.
- [ ] **Step 3: Implement.** Run the wasm check and clippy.
- [ ] **Step 4: Run the tests and the gate.** Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat: settings and bindings save themselves a second after the last change and at quit`.

### Task 10: Saves and settings in the browser

**Files:**
- Create: `src/core/saves/browser.rs` (the logic, compiled everywhere) and `src/core/saves/idb.rs` (`#[cfg(target_arch = "wasm32")]`, the real backend).
- Modify:
  - `Cargo.toml`: wasm32 web-sys features `IdbFactory`, `IdbOpenDbRequest`, `IdbDatabase`, `IdbObjectStore`, `IdbTransaction`, `IdbTransactionMode`, `IdbRequest`, `IdbVersionChangeEvent`, `IdbKeyRange`, `DomException`, `Event`, `EventTarget`; a direct `js-sys`.
  - `src/core/engine/{mod.rs, window.rs}`, `src/core/saves/AGENTS.md`.

**Interfaces:**
- **The backend seam:** `pub(crate) trait IdbBackend: Send + Sync { fn commit(&self, ops: Vec<IdbOp>, done: Box<dyn FnOnce(Result<(), IdbFailure>) + Send>); }`, with `IdbOp { Put(String, Vec<u8>), Delete(String) }` and `IdbFailure { Quota, Other(String) }`.
- **`BrowserStorage<B: IdbBackend>`** implements `Storage` over an in-memory mirror (`BTreeMap<String, Vec<u8>>`):
  - **`replace`** is one commit: `Put(name, new)`, plus `Put(backup, old)` when the old copy verifies. The mirror updates at once, and the commit returns a `Completion::Pending`.
  - **A failed commit** rolls the mirror back for those names. The error is `io::ErrorKind::StorageFull` for `Quota`, which the service maps to `SaveError::StorageFull`; otherwise `Other`.
  - **`recover`** reads the current copy, then the backup, from the mirror. The web has no temps.
- **The database:**
  - the database name comes from `store_name` (Task 6), with the object store `"files"`, keys as names and values as `Uint8Array`;
  - `pub(crate) async fn open_browser_store(name: &str) -> Result<BrowserStorage<IdbStore>, SaveError>` reads every entry.
- **The start-up gate:**
  - `pub(crate) struct StartupGate`: `gpu_ready(..)`, `store_ready(..)`, `ready() -> bool`;
  - `EngineEvent::StoreLoaded(Box<Result<BrowserStorage<IdbStore>, SaveError>>)`;
  - the web block of `resumed` spawns the store open alongside the GPU future, racing a 2 s timeout (`set_timeout` through js-sys) that resolves `Err(Unavailable)`;
  - `user_event` waits until both have arrived. It then installs the storage (`SaveService::set_storage`, or `Unavailable` with one warning), re-reads `system.json` into `Settings` (with `apply_file`, the applied snapshot, the volumes and `load_saved` bindings) and calls `finish_gpu_setup`.

- [ ] **Step 1: Write the failing tests** (native, with a fake `IdbBackend` that holds commits until the test releases them):
  - `writes_are_readable_at_once_and_finish_when_the_browser_commits`
  - `commits_apply_in_call_order`
  - `a_quota_failure_rolls_back_and_reports_storage_full`
  - `a_replace_keeps_the_previous_copy_as_backup`
  - `the_first_scene_waits_for_both_the_gpu_and_the_store` (`StartupGate`)
  - `a_store_that_does_not_open_leaves_saves_unavailable`: the service with an `Unavailable` storage gives `save` → `Unavailable`, and settings last for the session only.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p ryo2d --lib -- saves::browser`. Expected: FAIL.
- [ ] **Step 3: Implement** the logic and the wasm backend.
- [ ] **Step 4: Run the tests, the gate, and the wasm check and clippy.** Expected: PASS and clean.
- [ ] **Step 5: Commit** with the message `feat: saves and settings persist in the browser`.

### Task 11: Games, demos and skill examples move to the new API

**Files:**
- breakout:
  - `examples/games/breakout/src/save/mod.rs` and `src/scenes/{menu,game,pause}.rs`;
  - the tests `tests/{ball,cache,fragments,levels,lost_ball,menus,retain}.rs`. Remove the empty-folder `set_current_dir` helpers, since headless saves now stay in memory.
- flappy: `examples/games/flappy/src/{save.rs, scenes/menu.rs, scenes/game.rs}`.
- bullet-hell: `examples/games/bullet-hell/src/{save.rs, scenes/pause.rs, scenes/menu.rs, scenes/game_over.rs, scenes/game/mod.rs}`.
- The audio demo: `examples/demos/audio/src/buses.rs`, `examples/demos/audio/tests/pages.rs`.
- Skill examples: the compiled blocks that use `SaveManager`, `SystemState`, the audio volume-setting methods or `request_display_mode`:
  - `.claude/skills/ryo-config-save/SKILL.md`
  - `ryo-api` (the block at about line 1220)
  - `ryo-audio` (about 268 and 285-311)
  - `ryo-ui` (the block that opens at about 481)
  - `ryo-tilemap` (850-894)
  - Only the examples change here. Prose and version bumps wait for Task 14.

**Interfaces:**
- **High scores:** in breakout and flappy, `load_high_score(ctx: &mut SceneContext) -> u32` and `save_high_score(ctx: &mut SceneContext, score: u32)` use slot `"highscore"` through `ctx.saves()`, with the same `HighScoreData`.
- **Best time:** bullet-hell's `load_best_time(ctx)` and `save_best_time(ctx, f32)` use slot `"best_time"`.
- **Breakout's pause menu** holds no `SystemState`:
  - its sliders seed from `ctx.settings().volume(kind)`;
  - each change calls `ctx.settings().set_volume(kind, v)`;
  - `settings_changed` and the save in `resume` are removed, since volumes now persist on every way out.
- **Bullet-hell's pause menu** applies with `ctx.settings().set_display_mode(mode)`, reads the current mode from `ctx.settings().display_mode()`, and drops `save_display_mode`.
- **The audio demo's buses page** reads and writes `Settings`. `exit` still restores what it found. Its test reads `engine.world().resource::<Settings>().volume(VolumeKind::Master)`.

- [ ] **Step 1: Update the tests first.**
  - Add `the_high_score_survives_a_restart_of_the_scene` to breakout: a `MemorySaves` shared by two headless engines in turn.
  - Add `a_volume_set_in_the_pause_menu_is_kept_when_quitting_to_the_menu`.
  - Update the audio demo's test.
  - Run `cargo test -p breakout -p audio-demo` and expect FAIL.
- [ ] **Step 2: Move the games and the demo** to the new API.
- [ ] **Step 3: Rewrite the listed skill examples** to the new API. Run `SKILL_EXAMPLES=ryo-config-save,ryo-api,ryo-audio,ryo-ui,ryo-tilemap cargo check -p skill-examples`. Expected: clean.
- [ ] **Step 4: Run the gate and `cargo test --workspace`.** Expected: PASS.
- [ ] **Step 5: Commit** with the message `refactor: games, demos and skill examples save through ctx.saves() and ctx.settings()`.

### Task 12: The old saves and settings API is removed

**Files:**
- Delete: `src/core/config/save.rs`.
- Create: `src/core/config/preferences.rs`, taking `UserPreference`, `UserPreferenceFilter`, `DisplayMode`, `Resolution` and `parse_resolution` unchanged.
- Modify:
  - `src/core/config/{mod.rs, workspace.rs, resolve.rs, AGENTS.md}`;
  - `src/core/error.rs`, `src/lib.rs`;
  - `src/core/audio/{view.rs, param.rs, settings.rs, engine.rs, mixer.rs}` and their tests;
  - `src/core/scene/context.rs`, `src/core/window.rs`, `src/core/engine/{window.rs, tests.rs}`;
  - `src/core/ui/widgets/slider.rs` (doc example), `src/core/ecs/kernel.rs` (the alias comment);
  - `tests/integration/config_resolution.rs`, `tests/e2e/headless_engine.rs`.

**Interfaces:**
- **Removed:**
  - `SaveManager`, `GameSaveData`, `SystemState`, `compress_data`, `decompress_data`, `save_raw`/`load_raw`;
  - `Workspace::create_save_manager`, `SystemDefaults` (and `WorkspaceConfig.system`), `SaveManagerError`, `EngineError::Save`;
  - `AudioView::set_volume_setting`/`volume_setting`, and the same on `Audio` with `Queued::Setting`. `AudioEngine`'s `pub(crate)` setters stay;
  - `SceneContext::request_display_mode`, `request_resolution` and `request_display_change`;
  - `WindowRequest::with_resolution` and `with_display_mode`.
- **Narrowed:** `WindowRequest`'s `width`, `height` and `display_mode` become `pub(crate)`. `ui_scale` and `request_ui_scale` stay public.
- **Prelude:** it exports the moved types from `config::preferences`.

- [ ] **Step 1: Port the tests.**
  - `tests/integration/config_resolution.rs`: keep the resolution-parsing and filter tests, and move the file-format cases to the `settings::file` tests or delete them where Task 7 already covers them. List each deleted test and its replacement in the commit body.
  - `tests/e2e/headless_engine.rs:145-167`: the headless engine writes no `system.json` (now about the memory store).
  - The audio tests that called the removed methods now call the `AudioEngine` setters.
  - The rustdoc example in `audio/settings.rs` becomes a `Settings` example.
- [ ] **Step 2: Remove the API,** move the preference types and fix every importer.
- [ ] **Step 3: Run the gate, `cargo test --workspace`, `cargo test --doc -p ryo2d`, and the wasm check and clippy.** Expected: PASS and clean.
- [ ] **Step 4: Rewrite `src/core/config/AGENTS.md`** for the final state: the layers, the table, `ProjectFile`, and where settings and saves now live.
- [ ] **Step 5: Commit** with the message `refactor: the old save manager, settings state and window/volume setters are removed`.

### Task 13: The saves demo

**Files:**
- Create `examples/demos/saves/`, laid out like `examples/demos/audio`:
  - `Cargo.toml` (lib `saves_demo`, bin `saves-demo`; serde with derive), `Ryo.toml`, `Trunk.toml`, `index.html`;
  - `src/{main.rs, lib.rs, slots.rs, upgrades.rs, recovery.rs, settings_page.rs, shared.rs}`;
  - `tests/pages.rs`.
- The `Ryo.toml` sets `[project] name = "Ryo2D Saves Demo"` and `[release] save_folder = "Ryo2D/SavesDemo"`, with the kept preferences declared through the builder.
- Modify the `main.rs` of the seven web demos (`audio`, `character`, `lighting`, `look`, `physics`, `text`, `tilemap`) to add `.configure(|b| b.project(ryo2d::project!()))`.

**Interfaces:**
- `pub fn demo() -> Demo` registers:
  - `save_versions::<HeroV3>(SaveVersions::new(3).step(1, |v: HeroV1| -> HeroV2).step(2, |v: HeroV2| -> HeroV3))`;
  - `game_settings::<DemoOptions>("demo")`, where `DemoOptions { subtitles: bool, difficulty: String }`;
  - `with_user_preferences(UserPreferenceFilter::all())`;
  - `project(ryo2d::project!())`.
- demo-kit's own `Settings` type shadows the engine's in `demo_kit::prelude`. Reach the engine's through `ctx.settings()`, or name it `ryo2d::prelude::Settings`.

Pages:
1. **Slots.** Save, load and delete `slot-a`, `slot-b` and `slot-c` (a `HeroV3` with random stats), each listed with its header (name, time saved, play time, labels) and from-backup marks. `SaveFinished` results show in a log line.
2. **Upgrades.** On enter, if `hero-old` is missing, save a `HeroV1` there, as an older build would (ruling: there's no import API). Then `load::<HeroV3>("hero-old")`, showing the before and after and `upgraded_from`.
3. **Recovery** (native only; the web shows a note). The buttons:
   - damage `slot-a` by writing garbage over `<save dir>/slot-a.ryo-save` through `std::fs`, with the folder from `Workspace::discover()`, then load it to show the backup;
   - damage `system.json`, explaining that it will be set aside at the next start, and show whether `system.json.damaged` exists.
4. **Settings.** Volume sliders, display mode, vsync, FPS, and `DemoOptions` toggles. On native, the text of `system.json` refreshes once a second. On the web, the kept values show.

- [ ] **Step 1: Write the failing tests** (`HeadlessEngine::new_or_skip(demo().start_page(n).engine_builder())`):
  - `every_page_runs_two_seconds_without_a_warning`
  - `the_slots_page_saves_lists_and_deletes`: through `HeadlessEngine::saves()`.
  - `the_upgrades_page_loads_version_three_from_version_one`
  - `the_recovery_page_loads_the_backup_after_damage`: native, with `HeadlessEngine::saves()` damaged directly.
  - `the_settings_page_slider_changes_settings`
  - `the_web_build_embeds_every_asset_file`: if the demo has assets.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p saves-demo`. Expected: FAIL.
- [ ] **Step 3: Build the demo.** Then run `./dev.sh demos:tour saves` and look at the four PNGs in `target/demo-tour/saves/`.
- [ ] **Step 4: Run the gate, `cargo test --workspace`, and `cargo check -p saves-demo --target wasm32-unknown-unknown`.** Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat: a saves demo shows slots, upgrades, recovery and live settings`.

### Task 14: Skills, module notes and the engine overview

**Files:**
- Skills, each bumped once by a minor version:
  - `ryo-config-save` (2.4.3 → 2.5.0) is rewritten around `ctx.saves()`, `SaveVersions`, `Settings`, game sections, `system.json` format 3, `ryo2d::project!()` for the web, and the limits;
  - `ryo-api` (2.23.0 → 2.24.0)
  - `ryo-audio` (2.5.4 → 2.6.0)
  - `ryo-ui` (4.8.1 → 4.9.0): the settings-screen pattern
  - `ryo-controller` (2.1.1 → 2.2.0)
  - `ryo-game-dev` (2.9.0 → 2.10.0)
  - `ryo-tilemap` (1.8.0 → 1.9.0)
  - `ryo-cli` (1.3.0 → 1.4.0): the `system.json` sentence
  - `ryo-engine-dev` (2.5.2 → 2.6.0): the module table gains `diagnostics/`, `saves/` and `settings/`
- Module notes: `src/core/AGENTS.md`, `src/core/input/AGENTS.md`, `src/core/audio/AGENTS.md`, `src/core/tilemap/AGENTS.md` (SavedChunks with `ctx.saves()`), `examples/demos/AGENTS.md` (if it names web set-up: `ryo2d::project!()`).
- `docs/architecture/overview.md`, lines 35, 149, 242, 367-370 and 409 (the code map's list).

- [ ] **Step 1: Rewrite and update the text.** Grep for every removed name: `rg -n "SaveManager|SystemState|GameSaveData|create_save_manager|set_volume_setting|request_display_mode|compress_data|config::debug" .claude docs src examples`. Expected: no hits outside the spec, the plan and the changelog-like records.
- [ ] **Step 2: Check the examples.** Run `SKILL_EXAMPLES=ryo-config-save,ryo-api,ryo-audio,ryo-ui,ryo-controller,ryo-game-dev,ryo-tilemap,ryo-cli,ryo-engine-dev cargo check -p skill-examples`. Expected: clean.
- [ ] **Step 3: Commit** with the message `docs: skills, module notes and the overview describe saves, settings and project files`.

### Task 15: Measure and record

**Files:**
- Create: `benches/saves.rs` (criterion and `benches/guards`, as `benches/audio.rs` does), `docs/backlog/saves.md`.
- Modify: `Cargo.toml` (`[[bench]] name = "saves"`, `harness = false`).

Benches:
- `save_handoff_1kb` and `save_handoff_1mb`: the `save` call alone, guarded at a median of 20 µs. Build the data outside the timed part.
- `idle_settings`: one frame's settings work with no change (`apply_settings` plus the writer's `tick`), guarded at a median of 1 µs. Reach it through a `#[doc(hidden)] pub` hook in `ryo2d::testing`, if the benches have no other route.
- `frames_during_a_write`: a headless frame's time while a 1 MB save writes, against no write, interleaved A/B over three rounds. Recorded, with the verdict "within noise" or the measured difference.
- `load_100kb`, `load_1mb` and the on-disk size: recorded only.

- [ ] **Step 1: Write the bench.** Run it on a quiet machine, recording `uptime` before each run, three runs: `cargo bench --bench saves > <scratchpad>/bench-saves-N.log 2>&1`.
- [ ] **Step 2: Write `docs/backlog/saves.md`.**
  - A "Measured" section: the machine, `uptime`, every figure, and the guards' verdicts.
  - A "Limits" section:
    - on the web, a save's serialization runs on the frame;
    - closing a tab right after a save may lose the write;
    - native guarantees assume a local disk;
    - vsync is a no-op on the web;
    - a settings value equal to the default at save time leaves the file, so a later default change moves it;
    - every rotation reads the current file to verify it;
    - the web's first scene waits up to 2 s for the browser store.
  - Record only what the runs show; never write a cause you didn't measure.
- [ ] **Step 3: Commit** `benches/saves.rs`, `Cargo.toml` and `docs/backlog/saves.md` with the message `docs: saves and settings are measured, and their limits recorded`.
