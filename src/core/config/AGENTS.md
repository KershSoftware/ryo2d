# AGENTS.md - Config Module

See skill `ryo-config-save` for the Ryo.toml layout, the player's settings and the saves.

The project's configuration, and the types that say which of the player's saved settings a game uses. The player's settings (`system.json`) are in `core/settings` and the game's saves in `core/saves`; this module only supplies their defaults and filter.

| File | What |
|------|------|
| `workspace.rs` | `Workspace` (discovery, the save location, `from_toml_str`, the embedded workspace) and `WorkspaceConfig`: name, title, paths, `debug`, `release`, `tiled_project` and `[env]` (`env_json` gives it as JSON) |
| `project.rs`, `project_table.rs` | `ProjectLayer` and the table that declares each project setting (Project Settings, below) |
| `resolve.rs` | `resolve_settings` (pure): the layers into `ResolvedSettings`, and the engine's default values |
| `preferences.rs` | `UserPreference`, `UserPreferenceFilter`, `DisplayMode`, `Resolution` and `parse_resolution` (all in the prelude) |
| `project_file.rs` | `ProjectFile`, the Ryo.toml compiled into a game (Project File, below) |
| `save_location.rs` | Where release saves go and the browser store's name (Save Location, below) |

## Configuration Precedence

`EngineBuilder::resolve_in` (`engine/mod.rs`) calls `resolve::resolve_settings` (pure, unit-tested). Lowest to highest:
1. Engine defaults (`ResolvedSettings::defaults`)
2. Project values - `ProjectLayer` (Project Settings, below), from the first of: `RYO_EMBED_*` values from `ryo2d build` (release); the Ryo.toml `Workspace::discover()` found on disk (dev); the game's compiled-in Ryo.toml (`ProjectFile`, below: the web, or a native run with no Ryo.toml found); none
3. Builder calls - `BuilderOverrides`

Those are the game's defaults. `resolve_in` makes the player's `Settings` from them and applies `system.json` over the settings the (combined) `with_user_preferences` filter keeps, only keys present in the file (`settings/AGENTS.md`, In the engine).

- A missing system.json key is "no preference", not a default: the setting keeps the game's value, and the window size and an absent vsync or show_fps are never filled from the project. The file's format, its older formats (the version 1 volume conversion among them) and its writing are in `settings/AGENTS.md`
- `[system] shadow_quality` (`low`, `medium`, `high`, `ultra`) is the game's default shadow quality, and `EngineBuilder::shadow_quality` overrides it; a bad value warns and leaves `high`
- The engine never seeds `system.json`: the file keeps what the player changed, so a later builder or `[system]` change reaches every setting the player never touched. Headless builds read it from their memory store, never the disk
- Filter with `UserPreference` / `UserPreferenceFilter` (`all()`, `none()`, or an array of fields):
  - `Resolution` is the player's window size; the game's resolution (the pixel-art target or design area) always comes from the project and builder layers. Leave it out to keep the window the game picks
  - Audio-only settings: volume fields only

## Project Settings

Each project setting (a Ryo.toml key that becomes an engine setting) is declared once, as a line of `project_table.rs`: its `ProjectLayer` field and type, its section and key, its `RYO_EMBED_*` key (the logo has none) and a test sample unlike the default. The file is one `project_settings!` call, interpreted twice:
- `project.rs` generates `ProjectLayer` (every field an `Option`), `from_toml` (a value of the wrong type or text warns and stays unset), `from_embedded` (`None` without `RYO_EMBED_RESOLUTION`, the release marker; a malformed value warns and stays unset), `to_embedded_env`, `EMBED_KEYS` and their compiled values (`compiled_embedded_value`), `KNOWN_KEYS` and the test samples. The field types implement `TableValue` (from a Ryo.toml value, to and from embedded text)
- `src/build.rs` includes the same file with a `project_settings!` of its own that keeps only the embed keys, as build scripts can't import crate code

`workspace_keys` lists the other keys of the known sections, which `Workspace::build_config` reads itself; `env` ships `[env]` as JSON.

`Workspace::from_toml_str` is the one reader of Ryo.toml text, and loading from disk goes through it. Malformed TOML is a `WorkspaceError`. A key that a known section (`[project]`, `[paths]`, `[system]`, `[debug]`, `[release]`, `[tilemap]`) doesn't have warns, naming the key of that name in another known section (`console` under `[system]` is `debug.console`, the key it moved to), else the nearest known key of the section within three edits; `[env]` and the game's own sections are free-form. Warnings are logged once a run (`read_toml` hands them to a callback instead, for tests). The workspace keeps the layer as Ryo.toml sets it (`Workspace::project`) and fills `WorkspaceConfig` from it (`apply_project`).

`ProjectLayer::from_workspace` adds what a dev workspace fills in: the title's fallbacks (`[project] name`, then the folder's name), and the logo as a path from the root. `ryo2d build` (`embedded_config_env`) embeds that layer and the special values: `RYO_EMBED_RESOLUTION` always (1280x720 when unset), `RYO_EMBED_PORTABLE`, `RYO_EMBED_SAVE_FOLDER` (computed) and `RYO_EMBED_ENV`. It embeds only the `[debug]` values the project set: a build with the tools (`ryo2d build --dev-tools`) then takes the engine's defaults for the rest, as a dev run does, and a build without them ignores them all (Builds, below). A setting the project leaves unset ships as nothing and resolves to the engine's default, as in dev; the build's cargo command removes every embed key it doesn't set (`release_build_command`), so a `RYO_EMBED_*` variable left in the shell never ships. `TILED_PROJECT` empty and `AUDIO_BUFFER_FRAMES` 0 still mean none.

Adding a setting: its line in the table, a `ResolvedSettings` field that `resolve_settings` sets from the layer (and `BuilderOverrides` if it has a builder call), the copy in `EngineBuilder::resolve_in`, and `WorkspaceConfig::apply_project` if the public config shows it. `every_table_entry_reaches_the_resolved_settings` fails until `resolve_settings` uses it; the Ryo.toml reader, the embedding, `src/build.rs` and the misspelt-key check need nothing more.

## Project File

`ryo2d::project!()` (a proc macro in `ryo2d-derive`, exported at the crate root) compiles the calling crate's `Ryo.toml` (else `ryo.toml`, or the path it is given, relative to `CARGO_MANIFEST_DIR`) into the binary as a `ProjectFile` with `include_str!`, so cargo tracks the file; no file gives `ProjectFile::none()`. The game hands it to `EngineBuilder::project`. Its expansion names `::ryo2d::core::config::ProjectFile`, so it works only outside the `ryo2d` crate.

`resolve_in` reads the file only when there are no embedded values and no Ryo.toml on disk (`Workspace::has_config_file`), through `Workspace::from_toml_str`. It resolves as a release build of the same Ryo.toml: `ProjectLayer::for_release` is the one layer both `embedded_config_env` and the file use (resolution always set, no icon value). The one difference: a file has no folder to name an unnamed project after, so its title falls back to `[project] name` and then the engine's. `RYO_EMBED_RESOLUTION` stays the release marker for native code; a `ProjectFile` never switches it on. `a_web_style_resolve_matches_the_release_resolve_of_the_same_ryo_toml` (`cli/commands/build.rs`) holds the two paths together.

`save_location::store_name` names the browser's save store: `[release] save_folder` when valid, else `[project] name` made a folder name (`folder_from_project_name`), else the window title, warned once a process. `resolve_in` computes it into `ResolvedBuild`, and the web's `Engine` keeps it to open the browser's store (`saves/AGENTS.md`).

## Save Location

`save_location.rs` (pure, unit-tested) decides release paths (`valid_save_folder`, `release_save_dir`, `compiled_release_save_dir`, `bundle_dir_to_enter`, `enter_bundle_dir`). `Workspace::discover()` applies the release save dir (OS user folder + `[release] save_folder`/`[project] name`, or `data/` beside the game when portable); `from_path`/`discover_from` stay dev-style; `set_save_data_path` replaces it. `EngineBuilder::resolve` calls `enter_bundle_dir()` so release bundles run from their own folder. Embedded keys `RYO_EMBED_PORTABLE`/`RYO_EMBED_SAVE_FOLDER` feed it.

In release builds `Workspace::discover()` builds its config from embedded values (`Workspace::from_embedded`: `[system]`, `[debug]`, title, `[release]`, and `[env]` via the `RYO_EMBED_ENV` JSON) and never reads a Ryo.toml on disk, so `ryo2d run` matches a shipped game. `[tilemap] tiled_project` (`WorkspaceConfig::tiled_project`) ships as `RYO_EMBED_TILED_PROJECT`; an unset value ships nothing.

## Builds

The `[debug]` switches are developer tools' (the `dev_tools` cfg: debug builds, or the `dev-tools` feature). `DebugConfig`'s defaults follow the build: `dev_console` (`[debug] console`) and `dev_window_keys` are on with the tools and off without; Ryo.toml's `[debug]` turns each off (`the_console_and_dev_keys_default_on_with_tools`). `resolve_in` copies the resolved switches into the engine's `DebugConfig` through `apply_debug_settings` (`engine/mod.rs`), then applies the launch's overrides (`apply_launch_overrides`: `RYO_PROFILER_AUTO_OPEN`, the icon beside the game), which in a build without the tools end with `DebugConfig::switch_tools_off`, the last word: the console, the dev window keys, the logger window, the profiler, the collider, velocity and origin outlines, the frame timing, the stats and the UI debug overlays are off whatever the project or the builder set (`tests/no_tools.rs`, `a_project_cannot_switch_tools_on` and `a_launch_cannot_switch_the_profiler_on`, through the doc-hidden `testing::resolved_debug`). The FPS counter (`[system] show_fps`, the player's setting) and `disable_debug_keys` (F5) are not tools and stay. A plain `ryo2d build` ignores the rest of `[debug]` without a warning: the build has no tools for them to switch (`ryo2d build --dev-tools` keeps the tools, and says so). The `profiler` feature brings `dev-tools` with it (`Cargo.toml`), so a profiled build always has the tools.

## Gotchas

- One `DebugConfig` (`diagnostics/flags.rs`): `WorkspaceConfig::debug` holds the project's switches over its defaults (`[system] show_fps` also shows the FPS counter), and `resolve_in` copies the resolved ones into the engine's (Builds, above). `disable_debug_keys` is read once, at start
- `system.json` holds player preferences - don't commit it
- Frame timings: `Engine::frame` measures update (the UI pass and the fixed steps alone, from `update_start`: `First`, `PreUpdate` with the asset pump and map spawning, `Update`, `PostUpdate` with propagation, and `Last` count only in the total, so the three never add up to it), render and present (`FrameTimings`; present leaves out the renderer's wait for the window's next image, `WgpuRenderer::acquire_wait`, kept as `wait`), and at its end (`publish_frame_cost`) folds them into `SmoothedFrameTimings` (a plain mean until 50 frames, then an exponential average) and puts `FrameCost` (prelude, read-only accessors) in the world, windowed or headless. The F1 overlay draws the same smoothed values; the slow-frame warning in `after_frame` measures from the frame's start with waits and dev windows included
