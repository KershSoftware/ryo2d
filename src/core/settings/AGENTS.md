# AGENTS.md - Settings Module

The player's settings: the `Settings` resource and its file, `system.json`. The resource holds the engine's settings (the six volumes, the window size, the display mode, vsync and the FPS counter) and the game's own typed sections. The file keeps only the kept settings that differ from the game's defaults, so a default changed later in Ryo.toml or on the builder reaches every setting the player never touched. Files from older engines (formats 1 and 2) convert as they load. The code here is pure: nothing in it touches the engine, a world or the disk. The design is `docs/specs/2026-11-07-saves-and-settings-design.md` (where it and the code differ, the code is current).

| File | What |
|------|------|
| `mod.rs` | The exports (`Settings` is public and in the prelude; the rest is crate-internal) and `EngineValues`, the engine's settings as one value |
| `resource.rs` | `Settings`: the getters and sanitising setters, `reset`, `reset_all`, `is_kept`, `game`/`game_mut`, and for the engine `new`, `apply_file` and `to_file` |
| `sections.rs` | `SectionRegistry`: the game's sections by key and type, their versions, and how they load from and store into the file |
| `file.rs` | `SettingsFile`, `parse` (any format, to `Parsed`), `render` (format 3), and the conversions from formats 1 and 2. Its tests' `fixtures` module holds the format-1 and format-2 fixtures |

## The resource

`Settings` keeps the current values and the game's defaults (both `EngineValues`), the kept filter (`UserPreferenceFilter`, the game's `with_user_preferences`) and the sections.

- **Setters sanitise.** `set_volume` clamps to 0..=1 (`VolumeKind::sanitize`), and NaN is the game's default for that volume, not the engine's. `set_window_size` makes each side at least 1. `new` sanitises the defaults the same way.
- **Window size** is in logical pixels, like the builder's `window_size`.
- **`reset(preference)`** puts one setting back to the game's default; `reset(UserPreference::Bindings)` does nothing, since bindings live in the actions (`ctx.actions().reset_defaults()`). **`reset_all`** puts every engine setting back, and every section to its type's `Default`.
- **`game::<T>()` / `game_mut::<T>()`** find the section by type and panic for an unregistered one: `"Settings::game::<{type}>: register it with EngineBuilder::game_settings"`.
- **For the engine:** `Settings::new(defaults, kept, sections)` starts at the defaults. `apply_file(&file, &mut warnings)` applies the file's values for kept settings only (others last for the session) and loads the registered sections. `to_file(&base)` is the file to write: the kept values that differ from the defaults (a volume compared at two decimals, as the file keeps it), `base`'s bindings when `Bindings` is kept, the sections, and `base`'s unknown keys and unregistered sections as they were.

## Game sections

`SectionRegistry::register::<T>(key, versions)` takes a type that is `Default + Serialize + DeserializeOwned + Send + Sync + 'static` and, optionally, its `SaveVersions` (none is version 1); versions register into the registry's own `VersionRegistry`, so a gap panics at the call. A key or a type registered again replaces the first. A section starts at `T::default()`; a file without it leaves it there.

- **Stored shape:** `"game": { "<key>": { "version": N, "value": ... } }`. When `T` serializes to an object, `value` holds only the top-level fields that differ from `T::default()`'s; any other value is stored whole. A section with nothing changed is not written.
- **Loading** at the current version merges the stored fields over the default's object, then reads `T`. An older version's value goes to the steps (`VersionRegistry::decode_json`) as stored, since the default it was compared with is the older type's: an older section type should carry `#[serde(default)]`, because fields left at their default were never written. The next write stores the current version.
- **A section that does not read** (a newer version, a step that fails, a value of the wrong shape) keeps its value and adds the warning ``system.json: `game.{key}` has a bad value ({detail}); using the default``. The raw section is kept (`unread`) and written back while the section stays at its default, so a newer build's section survives this one; once the player changes the section, the current one is written. A section reads whole or not at all.
- **A section that does not serialize as JSON** (a map with non-string keys, say) warns once and keeps what the file held for it.

## The file (`system.json`)

`parse(bytes, project_env)` gives:

- `Damaged(reason)` when the bytes are not a JSON object (not JSON, not UTF-8, empty, an array);
- `Newer { file, warnings }` for a format above 3: the keys this engine knows are read, the rest kept as unknown, and the first warning says the file is newer;
- otherwise `Read { file, warnings }`.

`SettingsFile` holds the format read, the engine values (`FileValues`, `None` for a key absent or bad), the bindings, the raw sections (`RawSection { version, value }`) and the unknown keys (`serde_json::Map`), which are carried over on write.

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
- **`render`** writes format 3 whatever `file.format` says: pretty JSON (two spaces) ending in a line feed, keys in order: `format_version`, the engine keys (as above), `bindings` (when not empty), `game` (when not empty, each section's `version` before its `value`), then the unknown keys. It builds the text itself around `serde_json::Value`'s pretty printer, so it cannot fail and needs no `preserve_order`.

The fixtures are `tests/fixtures/saves/system-format1.json` and `system-format2.json` (their README says how they were made; never regenerate them).

## Testing

`resource.rs`'s tests: only a changed key is written; a setting set back to its default leaves the file; a project default changed later reaches a setting the player never set; volumes clamp and NaN is the default; both fixtures convert (format 1's volumes and `env`, format 2 keeping only `master_volume` and `env`); unknown keys and sections survive a write in the rendered order; a bad value warns and the rest applies; a setting not kept is neither read nor written; a section round-trips only its changed fields, upgrades with its steps, and is kept while it does not read; an unregistered section panics with the hint; `reset` and `reset_all`. `file.rs`'s: a newer format, damaged bytes, the legacy forms, and a full file rendering and reading back the same. `sections.rs`'s: a non-object section stored whole, and a second registration replacing the first.
