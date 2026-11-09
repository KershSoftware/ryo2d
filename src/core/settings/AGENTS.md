# AGENTS.md - Settings Module

The player's settings: the `Settings` resource and its file, `system.json`. The resource holds the engine's settings (the six volumes, the window size, the display mode, vsync and the FPS counter) and the game's own typed sections. The file keeps only the kept settings that differ from the game's defaults, so a default changed later in Ryo.toml or on the builder reaches every setting the player never touched. Files from older engines (formats 1 and 2) convert as they load, and the file saves itself a second after the last change to what it keeps. Nothing here touches the engine, a world or the disk: the settings writer (`persist.rs`) queues its writes on the save service, and the engine (`engine/`) reads the file through that service, applies what changes and drives the writer (In the engine, below). The design is `docs/specs/2026-11-07-saves-and-settings-design.md` (where it and the code differ, the code is current).

| File | What |
|------|------|
| `mod.rs` | The exports (`Settings` and `SettingsSaveFailed` are public and in the prelude; the rest is crate-internal) and `EngineValues`, the engine's settings as one value |
| `apply.rs` | `AppliedSettings` (the values the engine last applied, and the change check) and `diff`, which gives the `SettingsChanges` to apply |
| `resource.rs` | `Settings`: the getters and sanitising setters, `reset`, `reset_all`, `is_kept`, `game`/`game_mut`, and for the engine `new`, `values`, `has_sections`, `apply_file` and `to_file` |
| `sections.rs` | `SectionRegistry`: the game's sections by key and type, their versions, and how they load from and store into the file |
| `file.rs` | `SettingsFile`, `parse` (any format, to `Parsed`), `render` (format 3), the conversions from formats 1 and 2, the file's names (`FILE_NAME`, `DAMAGED_NAME`) and the storage's `verify`. Its tests' `fixtures` module holds the format-1 and format-2 fixtures |
| `persist.rs` | `SettingsWriter` (a world resource: writes the file a second after the last change, `DEBOUNCE`, and at once on a flush) and the `SettingsSaveFailed` message |

## The resource

`Settings` keeps the current values and the game's defaults (both `EngineValues`), the kept filter (`UserPreferenceFilter`, the game's `with_user_preferences`) and the sections.

- **Setters sanitise.** `set_volume` clamps to 0..=1 (`VolumeKind::sanitize`), and NaN is the game's default for that volume, not the engine's. `set_window_size` makes each side at least 1. `new` sanitises the defaults the same way.
- **Window size** is in logical pixels, like the builder's `window_size`.
- **`reset(preference)`** puts one setting back to the game's default; `reset(UserPreference::Bindings)` does nothing, since bindings live in the actions (`ctx.actions().reset_defaults()`). **`reset_all`** puts every engine setting back, and every section to its type's `Default`.
- **`game::<T>()` / `game_mut::<T>()`** find the section by type and panic for an unregistered one: `"Settings::game::<{type}>: register it with EngineBuilder::game_settings"`.
- **For the engine:** `Settings::new(defaults, kept, sections)` starts at the defaults; `values()` gives the engine's settings as one `EngineValues`. `apply_file(&file, &mut warnings)` applies the file's values for kept settings only (others last for the session) and loads the registered sections. `to_file(&base)` is the file to write: the kept values that differ from the defaults (a volume compared at two decimals, as the file keeps it), `base`'s bindings when `Bindings` is kept, the sections, and `base`'s unknown keys and unregistered sections as they were.

## Game sections

`SectionRegistry::register::<T>(key, versions)` takes a type that is `Default + Serialize + DeserializeOwned + Send + Sync + 'static` and, optionally, its `SaveVersions` (none is version 1); versions register into the registry's own `VersionRegistry`, so a gap panics at the call. A key or a type registered again replaces the first. A section starts at `T::default()`; a file without it leaves it there.

- **Stored shape:** `"game": { "<key>": { "version": N, "value": ... } }`. When `T` serializes to an object, `value` holds only the top-level fields that differ from `T::default()`'s, as long as merging them back over the default gives the value again. When they cannot express it, the entry is `{ "version": N, "whole": true, "value": <the whole value> }`: a field skipped while `None` (`skip_serializing_if`) whose default is `Some`, a key taken out of a map section, an enum section on another variant than its default's, a `#[serde(flatten)]` map; and an object whose default is not one (`Some` over a `None` default). A value that is not an object is stored as it is, with no `whole`, since a load merges only objects. A section equal to its default is not written.
- **Loading** at the current version merges the stored fields over the default's object, then reads `T`; a `whole` entry is read as it is, never merged. An older version's value goes to the steps (`VersionRegistry::decode_json`) as stored, since the default it was compared with is the older type's: an older section type should carry `#[serde(default)]`, because fields left at their default were never written. The next write stores the current version.
- **A section that does not read** (a newer version, a step that fails, a value of the wrong shape) keeps its value and adds the warning ``system.json: `game.{key}` has a bad value ({detail}); using the default``. The raw section is kept (`unread`) and written back while the section stays at its default, so a newer build's section survives this one; once the player changes the section, the current one is written. A section reads whole or not at all.
- **A section that does not serialize as JSON** (a map with non-string keys, say) warns once and keeps what the file held for it.

## The file (`system.json`)

`parse(bytes, project_env)` gives:

- `Damaged(reason)` when the bytes are not a JSON object (not JSON, not UTF-8, empty, an array);
- `Newer { file, warnings }` for a format above 3: the keys this engine knows are read, the rest kept as unknown, and the first warning says the file is newer;
- otherwise `Read { file, warnings }`.

`SettingsFile` holds the format read, the engine values (`FileValues`, `None` for a key absent or bad), the bindings, the raw sections (`RawSection { version, whole, value }`) and the unknown keys (`serde_json::Map`), which are carried over on write.

The keys:

| Key | Value |
|-----|-------|
| `format_version` | 3. Absent, or below 2: format 1 |
| `master_volume` ... `ambience_volume` | 0 to 1, written to two decimals |
| `resolution` | the window size, `"WxH"`, each side at least 1 |
| `display_mode` | `windowed`, `exclusive_fullscreen` or `borderless_fullscreen` (read case-insensitively) |
| `vsync`, `show_fps` | `true` or `false` |
| `bindings` | action name to a list of binding texts (`"Key:Space"`) |
| `game` | the sections, above |

- **A bad value** (the wrong type, a volume that is not finite or outside 0..=1, a size of 0) is left out with the warning ``system.json: `{key}` has a bad value ({detail}); using the default``, and the rest of the file applies. A bad binding list leaves out that action (`bindings.{action}`); a section of the wrong shape leaves out that section (`game.{key}`). `null` counts as absent, with no warning (format 2 wrote `"vsync": null` for unset).
- **Format 1** (before `format_version`): the volumes were amplitudes, heard at `20·log10(v)` dB; each becomes the setting heard at the same dB on today's curve, √v, to two decimals (0.25 becomes 0.5). A volume outside 0..=1 is bad before it converts.
- **Formats 1 and 2** also read the window size from flat `width` and `height`, or from the legacy `display` object (`resolution`, else `width` and `height`, plus `display_mode` and `vsync` where the flat keys are absent); the flat `resolution` comes first. Their `env` values that differ from the project's `[env]` become the section `env` (version 1), which a game reads by registering a section named `env`. These keys are consumed; in a format 3 file they are unknown keys and kept.
- **`render`** writes format 3 whatever `file.format` says: pretty JSON (two spaces) ending in a line feed, keys in order: `format_version`, the engine keys (as above), `bindings` (when not empty), `game` (when not empty, each section's `version`, then `whole` when true, then its `value`), then the unknown keys. It builds the text itself around `serde_json::Value`'s pretty printer, so it cannot fail and needs no `preserve_order`.

The fixtures are `tests/fixtures/saves/system-format1.json` and `system-format2.json` (their README says how they were made; never regenerate them).

## In the engine

**Start-up** (`EngineBuilder::resolve_in`, `engine/mod.rs`):

- **Defaults.** `resolve_settings` gives the resolved settings (engine defaults, then the project, then the builder); `player_defaults` turns them into the game's `EngineValues`. The default window size is the window the game opens (`initial_window_size` of the presentation, resolution and builder `window_size`), in logical pixels.
- **Kept.** The kept filter is every `with_user_preferences` filter combined. The builder's `game_settings::<T>(key)` and `game_settings_versioned::<T>(key, versions)` register sections into the builder's `SectionRegistry` at the call (so a gap in the versions panics there), and it moves into `Settings`.
- **The file is read** when the filter is not empty or a section is registered, through the save service the engine uses for saves, so a headless build reads its `MemorySaves` (the builder's `memory_saves`, else a fresh one) and never the disk; a native build reads the save folder's `FileStorage`. `resolve_in` makes the `SaveService` (with its storage) for this, and `Engine::new` puts it in the world.
- **Reading** (`read_settings_file`, giving a `SettingsRead`): `recover_named("system.json", file::verify)`, where a copy verifies when it is a JSON object. A damaged file (the current file not verifying) is set aside to `system.json.damaged` with `set_aside_named` (replacing an older one), and the newest whole copy is read instead: the backup when it verifies, else none, and the defaults are used. The engine warns which. A file that is gone (a crash between the replace's renames) is not set aside and not warned about. The parse's warnings (bad values, a newer format) and the sections' are logged; a file of a newer format is read like any other (`file.format` says it is newer). In the browser the set-aside commits later: its pending completion (`aside`) goes to the writer (Saving, below).
- **On the web** there is no storage at start, so this reads nothing; `Engine::store_arrived` reads the file again once the browser's store arrives, before the first scene, and applies it as below (saves module notes, The browser).
- **Applied:** `apply_file` over the defaults; the file's `bindings` go to `saved_bindings` (loaded into the actions in `Engine::new`) when `Bindings` is kept. The engine config takes the window size, display mode and FPS counter from `Settings`, `Engine::new` the volumes (onto the `AudioEngine` before it enters the world) and vsync. The file as read goes to the settings writer (`apply_settings_file`: `SettingsWriter::new`, then `after_set_aside`), so a write carries its unknown keys and sections over; a file of a newer format, or one `read_settings_file` left unread (it could not be read, or was damaged and could not be set aside), makes the writer read-only (Saving, below). An `[env]` the project has is the `project_env` an older file's `env` is compared with, as the old engine seeded the file from it.
- **Nothing is written at start-up:** a first run leaves no file.

**The resource** is inserted in `Engine::new` before the game's own resources; scenes reach it with `ctx.settings()` (`Mut<Settings>`), systems with `Res`/`ResMut<Settings>`.

**Each frame** `Engine::apply_settings` (`engine/frame.rs`) runs after the fixed steps (so after `Scene::update` and the UI pass), before the quit return, so a change in the last frame applies too:

- **The change check** is the frame's idle cost: `AppliedSettings::check` compares the resource's change tick with the world's tick at the last check, and the engine then moves the world's tick on, so a change made after the check (even outside a system, in the same tick) is newer. Only when it changed does `diff` compare the values with what was last applied.
- **Volumes** go to `AudioEngine::set_volume_setting` at once.
- **Display mode and window size** go into `WindowRequest`, applied in `about_to_wait` (`apply_window_request`, in `window_steps`' order: a return to a window before the size, so the size lands on the window; a fullscreen mode after it, since on Windows the last resize wins and a size after fullscreen would shrink it). The window size is converted to physical pixels with the engine's scale factor, and goes only when the new mode is windowed: while fullscreen it stays unapplied, and is asked for when the game makes the window windowed again.
- **Vsync** sets `Engine::pending_vsync`, which `about_to_wait` (and headless, `apply_headless_requests`) applies through `WgpuRenderer::set_vsync` before updating `Engine::vsync`. `set_vsync` reconfigures the surface with `present_mode_for` (on: `Fifo`; off: `Mailbox`, else `Immediate`, else `Fifo`, as `new` picks it) and updates `presentation_blocks`, which the frame pacing reads; an offscreen renderer and the web do nothing.
- **The FPS counter** sets the overlay flags as F1 does (`config.debug.show_fps`, then `OverlayMode::from_flags`), except in headless builds, whose frames carry no wall-clock overlays.

**The window into `Settings`** (`engine/window.rs`): the `Resized` arm reads `window.fullscreen()` (`display_mode_of`) and, while windowed and not minimised, the logical inner size, into `follow_os_window`. A mode that differs from the engine's goes through `set_display_mode`, the one funnel for the game's requests, F5 and the OS, which updates the engine, `WindowInfo` and `Settings` and the applied values together, so nothing is asked of the window again (no echo). The size goes into `Settings` (when it differs) and the applied values the same way. The web's canvas is sized by the page, not the player, so its size is not taken.

**Beside `Settings`:** `WindowRequest`'s width, height and display mode are crate-internal: the game asks for a window through `Settings`, and `WindowRequest` keeps only the UI scale (`ctx.request_ui_scale`) for the game.

## Saving (`persist.rs`)

`SettingsWriter` is made in `resolve_in` and put in the world by `Engine::new` when the build keeps anything (the filter is not empty or a section is registered: the rule the read follows). A build that keeps nothing has none, and never writes. A headless build writes into its `MemorySaves`, never the disk.

- **What it compares.** It holds the file as read (`base`: unknown keys, unregistered sections, and the bindings as last set) and texts of the file, each `render(settings.to_file(&base))` (so a volume compares at two decimals): `noted`, as `observe` last saw it; `stored`, the file as it stands; and the latest write queued, until its result. `stored` starts as the text of the start-up settings, so nothing is written until something changes, and an older file is converted at its first write.
- **A change.** `observe(now, &settings)` renders the file and calls `note_change(now)` when the text differs from `noted`. A change to a setting the file does not keep, or a game setting the same value every frame, notes nothing. With no section registered (`Settings::has_sections`), engine values equal to those `noted` was rendered from (`seen`) are no change and are not rendered, so a game setting a setting every frame costs a comparison. `set_bindings(map)` replaces the bindings to write (an empty map is none) and forgets `seen`, so the next `observe` renders.
- **The debounce.** `tick(now, ..)` writes once `now` is `DEBOUNCE` (1 s) past the last noted change (`is_due`): a slider dragged over 60 frames writes once, a second after it stops. A write renders again from the settings as they are, and queues nothing when the text equals the latest queued or stored one (a change undone within the second). It goes through `SaveService::write_named("system.json", text, file::verify)`: crash-safe, on the saves' writer, in order with the saves.
- **`flush_now`** writes at once, by the same comparison, from the settings as they are, so a change no frame has applied (the OS's resize as the window closes) is kept.
- **Results.** `drain_save_results` (`saves/view.rs`) gives each `system.json` result to `finished(id, result)`. A success of the latest write makes its text `stored`. A failure warns once a run (`warned`) and returns a `SettingsSaveFailed { error }`, which the system sends, for every failed write; a failed latest write is forgotten, so the next change or flush writes again (never every frame).
- **Read-only** (`read_only`): the file is never written this session, and the first write it would have made warns once (`warned`), naming the file as not fully read.
- **A set-aside still committing** (`aside`, the browser's): a write waits until it lands, keeping the change (`changed_at`) so the next tick tries again, and a flush writes nothing meanwhile. One that fails left the damaged file in place, so the writer becomes read-only and warns once.
- **Time** is `web_time::Instant`, passed in (game time pauses; tests use made-up instants).

**In the engine:**

- `Engine::apply_settings`, when `Settings` changed, calls `observe` at `Instant::now()`.
- `Engine::save_settings(now)` (`engine/frame.rs`) runs right after it each frame: the bindings changed since the last frame (`Engine::changed_bindings`: `Actions::take_dirty`, then `saved_map`) go to `set_bindings` and `observe`, then `tick`. An idle frame costs the writer's lookup, the bindings' dirty flag and `is_due`; the writer is reached with the settings and the saves through `Engine::with_settings_writer` (`try_resource_scope`, which moves the resource out and back) only when there is something to do.
- `Engine::flush_settings` (the changed bindings, then `flush_now`) runs in `ApplicationHandler::exiting` before `flush_saves` (which then waits for the settings write too), on `WindowEvent::Occluded(true)` (the web's page hidden, or a minimised or covered window), in `ApplicationHandler::suspended`, at the end of `run_offscreen_frames` before its saves flush, and in `Drop for Engine`. A write that fails at quit is not reported: no frame is left to drain it.

## Testing

`apply.rs`'s tests: a diff holds each changed setting and nothing else, and the change check sees a change once. `engine/tests.rs`: a volume set in `update` reaches the mixer that frame; each setting becomes its request (physical window size, the size held while fullscreen, vsync, the FPS overlay); `present_mode_for` and vsync reaching the engine after the frame; an OS fullscreen change and resize reach `Settings` with no echo; the player's file over the project for kept settings only; a damaged file set aside with the backup used (in memory and on disk); a headless engine reading its memory store, never the disk; a game section read in the first scene; a first run writing no file until a kept setting changes, then one write holding only that key; bindings landing in the same write as the settings, and a binding set back to its default leaving the file; quitting writing a pending change at once (a change no frame applied too, and before an offscreen run returns); a failed write warning once and sending `SettingsSaveFailed` each time; a newer file, and a damaged one that could not be set aside, never overwritten (in the browser too, where the set-aside commits later: no write until it lands); the browser's store arriving read into `Settings`, the mixer and the bindings; and a build that keeps nothing writing nothing. `engine/window.rs`: `display_mode_of`, and `window_steps`' order (a window before its size, a size before fullscreen).

`resource.rs`'s tests: only a changed key is written; a setting set back to its default leaves the file; a project default changed later reaches a setting the player never set; volumes clamp and NaN is the default; both fixtures convert (format 1's volumes and `env`, format 2 keeping only `master_volume` and `env`); unknown keys and sections survive a write in the rendered order; a bad value warns and the rest applies; a setting not kept is neither read nor written; a section round-trips only its changed fields, upgrades with its steps, and is kept while it does not read; sections the diff cannot express (a field skipped when `None`, a key removed from a map, another enum variant) round-trip whole; an unregistered section panics with the hint; `reset` and `reset_all`. `file.rs`'s: a newer format, damaged bytes, the legacy forms, and a full file rendering and reading back the same. `persist.rs`'s (over `MemorySaves`, with no engine): a slider drag over 60 frames writing once, a second after it stops; a value set again every frame writing once; a game section's change written like any other (`a_game_section_change_is_written_like_any_other`); a change undone within the second writing nothing; and a failed write tried again at the next flush, a landed one not written again. `sections.rs`'s: a non-object section stored whole, and a second registration replacing the first.
