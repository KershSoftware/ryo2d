# Saves and Settings Design

## Overview

**Intent.** A game's saves and a player's settings should be handled the way a professional engine handles them. A save never destroys the last good one. Old saves keep loading after the game changes. Saving never stalls a frame, and saves work in a browser too. Player settings apply the moment they change and save themselves, so a game never writes a settings file by hand. (Owner, 2026-11-07: "however a professional engine would handle it". The owner approved the three parts below as they were played back.)

**Why now.** This is the first of the six sub-projects left from the engine audit (`docs/research/2026-10-11-engine-audit.md`, "Config and save"). Checked against the code on 2026-11-07:

- **Writes can destroy data.** `config/save.rs` writes saves and `system.json` in place with `File::create` (lines 1109, 1231 and 1254). A crash, power cut or full disk mid-write leaves a cut-off file, and the previous save is already gone.
- **Any version change orphans every save.** The game-save header's version must equal `SAVE_VERSION` exactly (line 1280). There is no upgrade path, and the game's own data has no version at all.
- **Saving runs on the main thread**: serialising, compressing and writing all happen inside the frame.
- **No saves on the web.** `SaveManager::new` returns `Unsupported` in a browser, so web builds keep neither saves nor settings.
- **Games build their own access every time.** Breakout, flappy and bullet-hell call `Workspace::discover()` then `create_save_manager()` on every save and load.
- **Settings are split three ways.** A volume change applies through `ctx.audio().set_volume_setting(..)`, but the game must load, edit and write `system.json` itself to keep it (breakout's pause scene does). Display mode goes through `ctx.request_display_mode(..)`. Bindings save themselves, by reading and rewriting the whole file on each change (`engine/frame.rs`, `save_changed_bindings`).
- **The settings file freezes the defaults.** A new `system.json` is seeded with every filtered key, and from then on it overrides later Ryo.toml and builder changes. The module notes tell developers to delete it in dev to see a change.
- **One project setting lives in about seven places**: two `DebugConfig` structs (`workspace.rs` and `debug.rs`); `ProjectLayer`, `BuilderOverrides` and `ResolvedSettings` in `resolve.rs`; the copy in `EngineBuilder::resolve_in`; an `RYO_EMBED_*` key that is listed again in `resolve::COMPILED_EMBEDDED`, `src/build.rs` and `src/cli/commands/build.rs`. There are also three `system.json` readers (`load_system`, `load_or_create_system*` and `load_preferences_or_seed`).
- **Overlay drawing lives in config.** `config/debug.rs` (984 lines) draws the FPS counter, frame timings and stats, and holds the frame-cost types.

**Who it serves.** First, the player, whose progress and settings must survive crashes, updates and moves between machines. Second, the game developer, who saves a struct and loads it back with one call each, and changes settings by setting a value.

## Goals

- **Crash-safe saves with a backup.** At every instant, a complete copy of either the new save or the previous one exists, and a load finds it. Each save carries a checksum. A damaged save falls back to the backup, with a warning and a flag on the load result.
- **Versioned game data with upgrade steps.** Each saved type has a current version. The game supplies one typed step per older version, and loading an old save runs the steps in order. A save from a newer build is refused with an error naming both versions.
- **Saving off the main thread.** The frame that saves pays only for the hand-off. Serialising, compressing, checksumming and writing happen on a background thread. Completion arrives as a message carrying success or a typed error. On quit, the engine finishes pending writes.
- **Cheap slot lists.** A small header holds the save's name, time saved, play time, the game's labels and the data version. Listing slots reads only the headers.
- **Saves and settings in the browser**, on browser storage.
- **One way in:** `ctx.saves()` in scenes and a `Saves` system parameter in systems.
- **One live settings resource.** `ctx.settings()` (`ResMut<Settings>` in systems) holds the volumes, window size, display mode, vsync and FPS counter, plus the game's own typed sections. A change applies at once and is saved shortly after the last change, through the same crash-safe write.
- **The settings file keeps only what the player changed**, so a later default change in Ryo.toml reaches every setting the player never touched.
- **Gentle handling of damaged files.** A damaged settings file is set aside and defaults are used. A bad value falls back to its default, and the rest of the file is kept.
- **Each project setting is declared once.** One entry per setting drives Ryo.toml parsing, release embedding, builder overrides, the player file and the resolved value. A misspelt Ryo.toml key warns.
- **Web builds carry the project's values.** A web build resolves its settings from the game's Ryo.toml, as a release build does. Today it gets none.
- **Overlay drawing and frame timings move out of config** into a diagnostics module, ahead of the dev tools sub-project.
- **Existing player data survives.** Today's `.ryo-save` files (format 1) and today's `system.json` (formats 1 and 2) load, converted on first load.

## Non-goals

These may become later work:

- automatic snapshots of the ECS world (games save their own data; tile maps' `SavedChunks` already fits that);
- asynchronous loads (loads are synchronous; they happen on loading screens);
- autosave rotation helpers;
- screenshot thumbnails in slot headers;
- remembering the window's position;
- graphics quality presets;
- a "keep these display settings?" countdown (that is game UI);
- player profiles and multiple users;
- encryption, signing or anti-cheat;
- cloud-sync APIs (Steam Auto-Cloud syncs the save folder, so it already works);
- a CLI command to print a save as text.

## Anti-goals (the professional bar)

Any of these would be a failure even if everything else works:

- a crash at any point of a save leaving the slot unloadable when a good copy existed;
- a backup ever being replaced by a file that did not verify;
- a load misreading a damaged, newer or foreign file instead of refusing it with a clear error;
- an engine update or a game update making an existing save or settings file unreadable;
- any frame stall from saving that grows with the save's size (native);
- a settings slider drag writing the file every frame;
- the engine silently overwriting a settings file it could not fully read (newer or damaged);
- a game needing `Workspace::discover()`, a hand-built manager or hand-written settings JSON for anything in this design.

## Decisions

### The owner's decisions

- Handle saves and settings as a professional engine would (2026-11-07).
- Approved as played back: the game-save goals, the settings goals and the developer-side and acceptance goals above, including the non-goals listed with each.

### My calls (the owner may revisit them in review)

- **Saved types:** `save` takes the data by value (`T: Serialize + Send + 'static`), so serialisation also leaves the frame. A game builds a snapshot struct and hands it over.
- **Versions:** they are registered on the builder per saved type. A type that is never registered is version 1 with no steps. Steps are typed: each step converts the old version's type into the next version's. Small additions need no version bump, because `#[serde(default)]` covers them.
- **Body format:** compressed RON, kept from today. It handles any map key, and the body format matters to no one else. The checksum is CRC-32 (`flate2::Crc`, so no new dependency).
- **Loads are synchronous** and see pending writes: a load of a slot with a write queued returns the queued data.
- **Same-slot coalescing:** two saves to one slot before the first lands write only the later one. Both report finished.
- **Settings file name:** it stays `system.json`, at format 3.
- **Settings save timing:** one second after the last change, on quit, and on the web when the page is hidden.
- **Settings resource:** it replaces the old controls. `ctx.audio().set_volume_setting`/`volume_setting` (and the same on the `Audio` parameter), `ctx.request_display_mode` and `WindowRequest`'s width, height and display mode are removed. `WindowRequest` keeps `ui_scale`. Bindings stay in `ctx.actions()` and are written into the same file by the same writer.
- **Player window size:** in windowed mode, with `UserPreference::Resolution` kept, the window size the player drags to is saved.
- **`[env]`:** Ryo.toml's `[env]` stays as read-only project values (`config.env_*`) and is no longer copied into the player's file.
- **Web storage:** IndexedDB, mirrored in memory. It is loaded alongside the web's asynchronous GPU start-up, before the first scene, so loads stay synchronous. If the database doesn't open within 2 s, the game starts with saves `Unavailable`.
- **Project values on the web.** Web builds carry the game's Ryo.toml values, as release builds do. Today trunk web builds get none: `ryo2d build` has no web target, and `Workspace::discover()` fails on wasm. The values are compiled in from the game's own Ryo.toml, for trunk builds and `ryo2d build` alike, and the mechanism is the builder's. This keeps the declared-once table, default changes and the browser store's name the same on every platform.
- **The browser store's name** comes from `[release] save_folder`, else `[project] name`, else the builder's title. Falling back to the title warns once, because every game served from one origin (itch.io, `trunk serve` on localhost) shares that origin's storage. The name is permanent once a game ships: changing it orphans its players' web saves.
- **Slot names are lower case**: ASCII `a`-`z`, digits, `-` and `_`. The same name then means the same slot on case-insensitive disks (macOS, Windows), case-sensitive ones and in browser keys. A display name belongs in `SaveInfo`.
- **Older slot names stay reachable.** A native save whose name today's validator accepted but the new rule refuses (spaces, upper case, non-ASCII, inner dots) can still be loaded, listed and deleted under its existing name. New saves must follow the new rule.
- **The demo uses only public API.** The Recovery page damages files through the save folder's path, so it is native-only and shows a note on the web. The Settings page shows the file's text on native, and the current kept values on the web.
- **OS-driven window changes sync into `Settings`.** A fullscreen toggle through the OS or the engine's key, or a player's window resize, updates the resource, so it always reflects the window. Kept settings among these are saved like any other change.
- **Stale demo sections.** The `[display]`, `[window]` and `[game]` sections in eight demos' Ryo.toml files, which nothing reads, are removed, so the new misspelt-key warning starts clean.
- **Headless runs** keep saves and settings in memory: never on disk, never in `system.json`. `ryo2d::testing` can fill and read that store.
- **Diagnostics module:** the overlay and timing code moves to `src/core/diagnostics/`.
- **Misspelt keys:** a misspelt Ryo.toml key warns once and suggests the nearest known key.

## Design

### Saves

**Access.** `ctx.saves()` returns a `SavesView<'_>`, and the system parameter is `Saves`, following `ctx.audio()` and `Audio`. Both offer:

- **`save(slot, data)` and `save_with(slot, data, info)`.** `info` is a `SaveInfo` with a display name, play time and string labels (a `BTreeMap`). The call validates the slot and queues the write. It returns an error at once only for an invalid slot or when no storage is available. The result of the write arrives later as a `SaveFinished` message, carrying the slot and success or a shared `SaveError`.
- **`load::<T>(slot)`** returns `Loaded<T>`: the data, its header, whether it came from the backup, and the version it was upgraded from, if any.
- **Slot management:** `info(slot)` reads one header, `list()` gives every slot with its header, and there are also `exists(slot)` and `delete(slot)`. A delete removes the backup too, and is ordered after any pending write of that slot.
- **`is_writing(slot)`.**

**Slot names.** A slot name is 1 to 64 characters of lower-case ASCII letters, digits, `-` and `_`. Windows device names (`con`, `prn`, `aux`, `nul`, `com1`-`com9`, `lpt1`-`lpt9`) are refused. The same name is then the same slot on every disk, case-sensitive or not, and as a browser key. Older native saves under names today's validator accepted stay loadable, listable and deletable under those names (see My calls).

**Versions and upgrades.** The game registers each versioned type on `EngineBuilder`, with its current version and its steps:
- step 1 turns a version-1 value into version 2;
- the last step yields the current type;
- registration checks the steps run without gaps from 1 to current, and a gap is a build error.

On load:
- the header's version picks the starting type, and the steps run in order;
- a header newer than current is `SaveError::NewerVersion { found, current }`;
- an unregistered type saves and loads as version 1.

**Container (format 2).** The file starts with the magic `RYO2` and a format number. Next comes a header the engine can read without the body: data version, timestamp, play time, name, labels and the engine version that wrote it. The compressed body follows. A CRC-32 covers both header and body. The exact byte layout is the builder's.

Format 1 (today's: magic, version 1, deflated RON of `GameSaveData<T>`) is read and converted. Its header comes from its own fields, and it is version 1 of its type. The next save writes format 2.

**Writing on native.** Each save follows the same sequence:
1. Write a temporary file next to the slot.
2. Flush it to disk (`sync_all`).
3. Make the previous file the backup, then rename the temporary file over the slot.
4. On Unix, sync the directory afterwards.

The order must keep the invariant: at every instant, a complete, verified copy of either the new or the previous save exists, and load finds it. Load prefers a verified current file, then a verified newer temporary file, then the backup.

The backup is only ever replaced by a file known to verify, meaning one this run wrote or one checked on load or list. An unverified current file is checked before it is rotated. On Windows, the final rename is retried for about 100 ms, because a virus scanner may briefly hold the file. A leftover temporary file from a crash is ignored, and cleared by the next save of that slot.

**Background work.** Serialising, compressing, checksumming and writing run off the main thread, through the engine's worker threads or a dedicated save thread (the builder's choice). Writes to one slot happen in call order, and different slots may interleave. On quit, the engine waits for pending writes before exiting, warning if they take longer than a few seconds. On the web, which has no threads, the jobs run in the frame's job pump. That is a recorded limit: a big save costs its serialisation on the frame that runs it.

**The web.** The store lives in one IndexedDB database per game, holding the same container bytes per key. The database is named from `[release] save_folder`, else `[project] name`, else the builder's title with a warning (see My calls), and those values reach the web build at compile time. At start-up the engine reads the whole store into memory, in parallel with the asynchronous GPU set-up and before the first scene. Reads come from memory. Writes go to memory at once and to IndexedDB in a transaction, and `SaveFinished` arrives when the transaction completes. A full quota is `SaveError::StorageFull`. Where storage is unavailable (blocked, or some private modes), saves return `SaveError::Unavailable`, settings last only for the session, and the game keeps running. Limit: closing the tab right after a save may lose a write still in flight, because browsers don't wait for IndexedDB at unload.

**Storage seam.** Native files, browser storage and memory sit behind one crate-internal interface. The memory store doubles as the fault-injection store for tests (fail or cut off at a chosen step). `ryo2d::testing` exposes the headless memory store, so a test can plant an old or damaged save and read back what a game wrote.

### Settings

**The resource.** `Settings` is a resource the engine creates at start-up from the resolved settings and the player's file. Its fields are private, with getters and setters, and the setters sanitise: volumes are clamped to 0 to 1 and NaN means the default. It has:
- engine settings: `volume(VolumeKind)`/`set_volume`, `window_size`/`set_window_size`, `display_mode`/`set_display_mode`, `vsync`/`set_vsync`, `show_fps`/`set_show_fps`;
- `reset(..)` for one setting and `reset_all()`, back to the game's defaults;
- game sections: `game::<T>()` returns `&T` and `game_mut::<T>()` returns `&mut T`.

Scenes reach it with `ctx.settings()` (a `Mut<'_, Settings>`), and systems with `Res`/`ResMut<Settings>`.

**Game sections.** A game registers each section on `EngineBuilder` with a key, its type (`Default + Serialize + DeserializeOwned + Send + Sync + 'static`) and, optionally, versions with typed upgrade steps (the same mechanism as saves). A section the file lacks starts at `T::default()`.

**Applying.** After the scene's update each frame, an engine system compares the resource with what was last applied, and applies only the differences:
- volumes go to the audio engine's player volume settings;
- window size and display mode go to the window;
- vsync reconfigures the surface's present mode;
- the FPS setting toggles the overlay.

A frame with no change costs one change-detection check.

**Saving.** Only settings in the `with_user_preferences` filter (unchanged) and registered game sections are written. Everything else lasts for the session. Writing happens:
- one second after the last change to a kept setting;
- on quit or window close;
- on the web, when the page becomes hidden.

Bindings changed through `ctx.actions()` are written by the same writer, with the same delay. The writer uses the crash-safe path, and on the web the browser store. A failure warns once a run and sends a `SettingsSaveFailed` message. Headless runs never write.

**The file (`system.json`, format 3).** A JSON object holding:
- `format_version: 3`;
- only the kept engine settings whose value differs from the game's default at save time;
- `bindings`, unchanged from today: only bindings that differ from the defaults;
- the game sections, each with its own version.

A setting set back to its default leaves the file. Keys the engine does not know, written by a newer build or by hand, are kept as they are on write.

**Older files.** A format 1 or 2 file is converted on load:
- format 1's amplitude volumes go through the existing `RawSystemFile::upgrade`;
- keys equal to the current defaults are dropped, since the old seeding wrote every filtered key;
- an old `env` object's values that differ from Ryo.toml's `[env]` are kept as a raw section under the key `env`, which a game reads by registering a section named `env`.

The converted file is written at the first save.

**Newer and damaged files.** A file of a newer format has its known keys read with a warning, and is never overwritten in that session. A file that does not parse is renamed aside to `system.json.damaged` (replacing an older one), defaults are used, and the engine warns. A single bad value (wrong type, out of range) falls back to its default with a warning, and the rest of the file applies.

### Project settings declared once

One table has an entry per project setting, giving:
- its Ryo.toml section and key and its type;
- the engine default;
- whether a builder call sets it;
- its release embedding key;
- whether a player may keep it (its `UserPreference`, if any).

The Ryo.toml reader, the release embedding (`ryo2d build`, `src/build.rs`, the compiled lookup), the web build's embedding, the builder overrides and the resolved settings are all generated from or checked against that table. The build script can `include!` the table file. A web build resolves exactly as a release build does, from the values compiled in. The two `DebugConfig` structs become one, and the three `system.json` readers become one.

The precedence is unchanged: engine defaults, then Ryo.toml or the embedded values, then builder calls, then the player's file. Tests prove every layer covers every entry, generalising today's sync tests of the embedded keys.

**Misspelt keys.** An unknown key in a known Ryo.toml section warns once, naming the file and section and suggesting the nearest known key. The known sections are `[project]`, `[paths]`, `[system]`, `[debug]`, `[release]` and `[tilemap]`. `[env]` stays free-form.

### Diagnostics

`config/debug.rs` moves to `src/core/diagnostics/`:
- frame timings: `FrameTimings`, `SmoothedFrameTimings`, `FrameCost`, `time_block`;
- overlays: `OverlayMode`, the stats types, the colours and the FPS, timing and stats drawing.

The drawing functions become crate-internal. `FrameCost` stays in the prelude. Overlays draw pixel-identically to main.

### Errors

`SaveError` is a `thiserror` enum whose variants keep their cause (`#[source]`):
- `Io { path, source }`;
- `Corrupt { slot, reason }`;
- `NewerVersion { found, current }`;
- `NotFound`;
- `InvalidSlot`;
- `StorageFull`;
- `Unavailable`;
- `Serialize(source)`;
- `Deserialize { slot, source }`.

Messages carry it shared (`Arc`), since messages are cloned.

## Public API and migration

Breaking changes are fine (owner's standing rule).

- **Removed:**
  - `SaveManager`, `Workspace::create_save_manager`, `GameSaveData`, `SystemState`, `compress_data`, `decompress_data`, `save_raw`/`load_raw`;
  - the audio volume-setting methods, `ctx.request_display_mode`, and `WindowRequest`'s width, height and display mode;
  - the public overlay drawing functions.
- **Added:**
  - saves: `SavesView`, `Saves`, `SaveInfo`, `Loaded<T>`, `SaveError`, `SaveFinished`, the builder's version registration;
  - settings: `Settings`, `SettingsSaveFailed`, the builder's game-section registration;
  - tests: `ryo2d::testing` access to the memory store.
- **Kept:** `Workspace`, `WorkspaceConfig`, `UserPreference`/`UserPreferenceFilter`, `with_user_preferences`, `DisplayMode`, `Resolution`, `FrameCost`.
- **Games:** breakout (high score; pause-menu volumes), flappy (high score) and bullet-hell (best time; display mode) move to `ctx.saves()` and `ctx.settings()`. The audio demo's bus page moves its volume sliders to `Settings`.

## Acceptance

### Correctness (tests)

- **Interrupted writes.** With the fault store cutting a save off at each step in turn, every following load returns the previous or the new data, whole. Never an error when a good copy existed, never a mix.
- **Damage.** A cut-off file, a flipped byte in the header and a flipped byte in the body each load the backup, with the flag set and a warning. When both copies are damaged, the load returns `Corrupt` naming the slot. A backup is never replaced by an unverified file.
- **Versions.** A version-1 save upgrades through two steps to version 3. A newer save is refused as `NewerVersion`. A gap in the steps fails at build. An unregistered type round-trips as version 1.
- **Old files.** A format-1 `.ryo-save` fixture, written by today's code and committed before the change, loads. `system.json` fixtures at formats 1 and 2 convert as specified.
- **Ordering.**
  - A load while a write is pending returns the pending data.
  - Two saves to one slot in a frame write the later and report both finished.
  - A delete after a pending save leaves no slot.
  - Pending writes finish before a headless engine exits, tested with a slow store.
- **Slot names:** traversal, empty, too long and Windows device names are refused.
- **Settings.**
  - A volume change reaches the mixer that frame.
  - A display mode change reaches the window request, and vsync the renderer's present mode.
  - A slider drag over 60 frames writes the file once.
  - The file holds only changed keys.
  - A default changed in Ryo.toml reaches a setting the player never set.
  - A damaged file is set aside, a bad value falls back, and a newer format is not overwritten.
  - Bindings land in the same file, and game sections round-trip and upgrade.
  - Headless runs never touch disk.
- **Project settings:** every table entry is covered by the Ryo.toml reader, the embedding, the builder and the resolver. A misspelt key warns with a suggestion.
- **The web.** `./dev` web check and clippy pass for the wasm target. The browser store's mirror logic is tested natively against a fake asynchronous store. A trunk web build resolves the same project values as a release build of the same Ryo.toml, and names its store from them. The saves demo is checked by hand in a browser: save, reload the tab and load on the Slots and Upgrades pages, and change a setting, reload, and see it kept.
- **Overlays:** the demo tour's overlay pages match main pixel for pixel.

### Targets (record machine, quiet, `uptime` recorded)

- **`save` on the main thread:** ≤ 20 µs for any data size (native).
- **Frames during a write:** a 1 MB save writing in the background changes frame time by no more than noise, by interleaved A/B.
- **Idle settings:** a frame with no settings change costs ≤ 1 µs for settings.
- **Recorded, with no target:** load time of 100 KB and 1 MB saves, and on-disk size.

### Demo

`examples/demos/saves`, on the demo-kit standard, runs native and web, using only public API. It has four pages:
1. **Slots:** save, load and delete three slots, listed with their headers.
2. **Upgrades:** a version-1 save shipped with the demo loads into version 3, showing what each step changed.
3. **Recovery** (native only; the web shows a note): buttons damage a save or the settings file through the save folder's path, then load to show the backup or the set-aside file.
4. **Settings:** volume sliders, display mode, vsync and FPS, plus a game section. On native the file's text is shown live, and on the web the kept values.

## Documentation

- **`ryo-config-save`:** rewritten around `ctx.saves()`, versions, `Settings` and the file.
- **Other skills:** `ryo-api`, `ryo-audio` (volume settings), `ryo-ui` (the settings screen pattern), `ryo-controller` (saved bindings) and `ryo-game-dev` are updated. Each skill is bumped by a minor version, once.
- **Module notes:** the config notes are rewritten, and the diagnostics module gets its own.
- **Engine overview:** `docs/architecture/overview.md` is updated where it names config or saves.

## Risks

- **IndexedDB through `web-sys`** is verbose and asynchronous, and the start-up join with the GPU set-up must not delay a game without saves noticeably.
- **Live vsync** depends on reconfiguring the surface on every backend. A backend that refuses it is logged, and the change applies at next start.
- **Windows renames** can fail while another process holds the file. The retry covers scanners, not a file another program keeps open.
- **Network folders** can make directory syncs and renames slower or non-atomic. Native guarantees assume a local disk.

## Details settled at planning (2026-11-07, my calls)

The code maps made at planning settled these. The owner may revisit any of them.

- **Damaged settings file.** It is set aside to `system.json.damaged`. The settings backup is used if it parses; otherwise the defaults are.
- **Headless runs** read and write settings in their memory store as well (never on disk), so tests can watch the file.
- **Backup rotation** always reads and verifies the current file first. A slot list's header check does not count as verified.
- **Temporary files** are named `<name>.<pid>-<n>.tmp`, so a game launched twice cannot interleave writes. Recovery picks the newest verified temp, by its header's save time.
- **Gaps in version steps.** `EngineBuilder::build` returns an `Engine`, not a `Result`, so a gap panics at the `save_versions` or `game_settings_versioned` call, naming the type and the missing step.
- **A newer container format** is reported as `NewerVersion { found, current: 2 }`.
- **`SaveFinished`** carries `op: SaveOp { Save, Delete }`.
- **A failed directory sync** after the final rename warns and reports success, since the data is in place.
- **Window size in `Settings`** is in logical pixels, like the builder's `window_size`.
- **Old error type.** Until the old API is removed, the old `SaveError` is renamed `SaveManagerError`.
- **Game sections** store only their top-level fields that differ from `T::default()`, as `"game": { "<key>": { "version": N, "value": {…} } }`.
- **Page-hidden flush.** The settings writer also flushes on `Occluded(true)` (the web's `visibilitychange`, and minimising or covering on native) and on `suspended`.
- **Web mechanism.** `ryo2d::project!()` is a proc macro in `ryo2d-derive`, and games pass its result to `EngineBuilder::project(..)`. It reads the calling crate's Ryo.toml at compile time. Native release builds keep the `RYO_EMBED_*` values.
- **The project table** is one file, `src/core/config/project_table.rs`. The crate interprets it, and `src/build.rs` includes it with its own macro to list the embed keys. `ResolvedSettings` and the builder's overrides stay hand-written, and a test checks that every entry reaches them.
- **The saves demo's Upgrades page** writes its version-1 save from the old struct, as an older build would. There is no import API, so this keeps the page on public API on the web as well.
