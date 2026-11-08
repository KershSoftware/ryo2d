# AGENTS.md - Config Module

See skill `ryo-config-save` for Ryo.toml layout and the save API.

## Configuration Precedence

`EngineBuilder::resolve_in` (`engine/mod.rs`) calls `resolve::resolve_settings` (pure, unit-tested). Lowest to highest:
1. Engine defaults (`ResolvedSettings::defaults`)
2. Project values - `ProjectLayer` (Project Settings, below), from the first of: `RYO_EMBED_*` values from `ryo2d build` (release); the Ryo.toml `Workspace::discover()` found on disk (dev); the game's compiled-in Ryo.toml (`ProjectFile`, below: the web, or a native run with no Ryo.toml found); none
3. Builder calls - `BuilderOverrides`
4. `system.json` preferences - only with `.with_user_preferences(filter)`, only keys present in the file and in the (combined) filter - `StoredPreferences`

- A missing system.json key is "no preference", not a default. `SaveManager::load_or_create_system` (and `_filtered`) return the game's `[system]` value for a missing volume or display mode (as the engine plays it), `load_system` the engine's; the window size and an absent vsync or show_fps are never filled from the project
- `system.json` carries `format_version` (`SYSTEM_FORMAT`, 2; written by every save). A file without it is format 1, from an engine whose volumes were amplitudes (20·log10 v dB): `RawSystemFile::upgrade` converts each saved volume to √v (the same dB on the 40·log10 v curve, two decimals) as any load reads it, and the loads that write (`load_or_create_system*`, the engine's startup read) save it with the version at once, as the file loads with the engine's defaults, so the save adds no key. That save is best effort (`save_loaded_system`): a file that cannot be written (read-only, on read-only media) still loads, converted, warned once a run, and is converted again at each load. A file is converted once; `load_system` converts only what it returns
- A new system.json is seeded from levels 1-3; headless builds never read or write it
- After the first run, `data/system.json` holds the filtered settings and overrides later builder or `[system]` changes - delete it in dev to re-seed.
- Filter with `UserPreference` / `UserPreferenceFilter` (`all()`, `none()`, or an array of fields):
  - `Resolution` is the player's window size; the game's resolution (the pixel-art target or design area) always comes from the project and builder layers. Leave it out to keep the window the game picks
  - Audio-only settings: volume fields only

## Project Settings

Each project setting (a Ryo.toml key that becomes an engine setting) is declared once, as a line of `project_table.rs`: its `ProjectLayer` field and type, its section and key, its `RYO_EMBED_*` key (the logo has none) and a test sample unlike the default. The file is one `project_settings!` call, interpreted twice:
- `project.rs` generates `ProjectLayer` (every field an `Option`), `from_toml` (a value of the wrong type or text warns and stays unset), `from_embedded` (`None` without `RYO_EMBED_RESOLUTION`, the release marker; a malformed value warns and stays unset), `to_embedded_env`, `EMBED_KEYS` and their compiled values (`compiled_embedded_value`), `KNOWN_KEYS` and the test samples. The field types implement `TableValue` (from a Ryo.toml value, to and from embedded text)
- `src/build.rs` includes the same file with a `project_settings!` of its own that keeps only the embed keys, as build scripts can't import crate code

`workspace_keys` lists the other keys of the known sections, which `Workspace::build_config` reads itself; `env` ships `[env]` as JSON.

`Workspace::from_toml_str` is the one reader of Ryo.toml text, and loading from disk goes through it. Malformed TOML is a `WorkspaceError`. A key that a known section (`[project]`, `[paths]`, `[system]`, `[debug]`, `[release]`, `[tilemap]`) doesn't have warns, naming the nearest known key within three edits; `[env]` and the game's own sections are free-form. Warnings are logged once a run (`read_toml` hands them to a callback instead, for tests). The workspace keeps the layer as Ryo.toml sets it (`Workspace::project`) and fills `WorkspaceConfig` from it (`apply_project`).

`ProjectLayer::from_workspace` adds what a dev workspace fills in: the title's fallbacks (`[project] name`, then the folder's name), no console unless `[system] console` turns it on, and the logo as a path from the root. `ryo2d build` (`embedded_config_env`) embeds that layer and the special values: `RYO_EMBED_RESOLUTION` always (1280x720 when unset), `RYO_EMBED_DEV_WINDOW_KEYS` from an explicit value only (else `false`), `RYO_EMBED_PORTABLE`, `RYO_EMBED_SAVE_FOLDER` (computed) and `RYO_EMBED_ENV`. A setting the project leaves unset ships as nothing and resolves to the engine's default, as in dev; the build's cargo command removes every embed key it doesn't set (`release_build_command`), so a `RYO_EMBED_*` variable left in the shell never ships. `TILED_PROJECT` empty and `AUDIO_BUFFER_FRAMES` 0 still mean none.

Adding a setting: its line in the table, a `ResolvedSettings` field that `resolve_settings` sets from the layer (and `BuilderOverrides` if it has a builder call), the copy in `EngineBuilder::resolve_in`, and `WorkspaceConfig::apply_project` if the public config shows it. `every_table_entry_reaches_the_resolved_settings` fails until `resolve_settings` uses it; the Ryo.toml reader, the embedding, `src/build.rs` and the misspelt-key check need nothing more.

## Project File

`ryo2d::project!()` (a proc macro in `ryo2d-derive`, exported at the crate root) compiles the calling crate's `Ryo.toml` (else `ryo.toml`, or the path it is given, relative to `CARGO_MANIFEST_DIR`) into the binary as a `ProjectFile` with `include_str!`, so cargo tracks the file; no file gives `ProjectFile::none()`. The game hands it to `EngineBuilder::project`. Its expansion names `::ryo2d::core::config::ProjectFile`, so it works only outside the `ryo2d` crate.

`resolve_in` reads the file only when there are no embedded values and no Ryo.toml on disk (`Workspace::has_config_file`), through `Workspace::from_toml_str`. It resolves as a release build of the same Ryo.toml: `ProjectLayer::for_release` is the one layer both `embedded_config_env` and the file use (resolution always set, dev window keys off unless set, no icon value). The one difference: a file has no folder to name an unnamed project after, so its title falls back to `[project] name` and then the engine's. `RYO_EMBED_RESOLUTION` stays the release marker for native code; a `ProjectFile` never switches it on. `a_web_style_resolve_matches_the_release_resolve_of_the_same_ryo_toml` (`cli/commands/build.rs`) holds the two paths together.

`save_location::store_name` names the browser's save store: `[release] save_folder` when valid, else `[project] name` made a folder name (`folder_from_project_name`), else the window title, warned once a process. `resolve_in` computes it into `ResolvedBuild` and the `Engine` keeps it.

## Save Location

`save_location.rs` (pure, unit-tested) decides release paths (`valid_save_folder`, `release_save_dir`, `compiled_release_save_dir`, `bundle_dir_to_enter`, `enter_bundle_dir`). `Workspace::discover()` applies the release save dir (OS user folder + `[release] save_folder`/`[project] name`, or `data/` beside the game when portable); `from_path`/`discover_from` stay dev-style; `set_save_data_path` replaces it. `EngineBuilder::resolve` calls `enter_bundle_dir()` so release bundles run from their own folder. Embedded keys `RYO_EMBED_PORTABLE`/`RYO_EMBED_SAVE_FOLDER` feed it.

In release builds `Workspace::discover()` builds its config from embedded values (`Workspace::from_embedded`: `[system]`, `[debug]`, title, `[release]`, and `[env]` via the `RYO_EMBED_ENV` JSON) and never reads a Ryo.toml on disk, so `ryo2d run` matches a shipped game. `[tilemap] tiled_project` (`WorkspaceConfig::tiled_project`) ships as `RYO_EMBED_TILED_PROJECT`, empty for none.

## Gotchas

- One `DebugConfig` (`diagnostics/flags.rs`): `WorkspaceConfig::debug` holds the project's switches over its defaults (`[system] show_fps` also shows the FPS counter; the console is off unless `[system] console` turns it on), and `resolve_in` copies the resolved ones into the engine's. `disable_debug_keys` is read once, at start
- `system.json` holds player preferences - don't commit it
- Frame timings: `Engine::frame` measures update (the UI pass and the fixed steps alone, from `update_start`: `First`, `PreUpdate` with the asset pump and map spawning, `Update`, `PostUpdate` with propagation, and `Last` count only in the total, so the three never add up to it), render and present (`FrameTimings`; present leaves out the renderer's wait for the window's next image, `WgpuRenderer::acquire_wait`, kept as `wait`), and at its end (`publish_frame_cost`) folds them into `SmoothedFrameTimings` (a plain mean until 50 frames, then an exponential average) and puts `FrameCost` (prelude, read-only accessors) in the world, windowed or headless. The F1 overlay draws the same smoothed values; the slow-frame warning in `after_frame` measures from the frame's start with waits and dev windows included
