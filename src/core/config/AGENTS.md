# AGENTS.md - Config Module

See skill `ryo-config-save` for Ryo.toml layout and the save API.

## Configuration Precedence

`EngineBuilder::resolve_in` (`engine/mod.rs`) calls `resolve::resolve_settings` (pure, unit-tested). Lowest to highest:
1. Engine defaults (`ResolvedSettings::defaults`)
2. Project values: Ryo.toml (dev) or `RYO_EMBED_*` values from `ryo2d build` (release) - `ProjectLayer`
3. Builder calls - `BuilderOverrides`
4. `system.json` preferences - only with `.with_user_preferences(filter)`, only keys present in the file and in the (combined) filter - `StoredPreferences`

- A missing system.json key is "no preference", not a default. `SaveManager::load_or_create_system` (and `_filtered`) return the game's `[system]` value for a missing volume or display mode (as the engine plays it), `load_system` the engine's; the window size and an absent vsync or show_fps are never filled from the project
- `system.json` carries `format_version` (`SYSTEM_FORMAT`, 2; written by every save). A file without it is format 1, from an engine whose volumes were amplitudes (20·log10 v dB): `RawSystemFile::upgrade` converts each saved volume to √v (the same dB on the 40·log10 v curve, two decimals) as any load reads it, and the loads that write (`load_or_create_system*`, the engine's startup read) save it with the version at once, as the file loads with the engine's defaults, so the save adds no key. That save is best effort (`save_loaded_system`): a file that cannot be written (read-only, on read-only media) still loads, converted, warned once a run, and is converted again at each load. A file is converted once; `load_system` converts only what it returns
- A new system.json is seeded from levels 1-3; headless builds never read or write it
- After the first run, `data/system.json` holds the filtered settings and overrides later builder or `[system]` changes - delete it in dev to re-seed.
- Embedded keys live in `resolve::COMPILED_EMBEDDED`, `cli/commands/build.rs::embedded_config_env` and `src/build.rs` (tests keep them in sync)
- Filter with `UserPreference` / `UserPreferenceFilter` (`all()`, `none()`, or an array of fields):
  - `Resolution` is the player's window size; the game's resolution (the pixel-art target or design area) always comes from the project and builder layers. Leave it out to keep the window the game picks
  - Audio-only settings: volume fields only

## Save Location

`save_location.rs` (pure, unit-tested) decides release paths (`valid_save_folder`, `release_save_dir`, `compiled_release_save_dir`, `bundle_dir_to_enter`, `enter_bundle_dir`). `Workspace::discover()` applies the release save dir (OS user folder + `[release] save_folder`/`[project] name`, or `data/` beside the game when portable); `from_path`/`discover_from` stay dev-style; `set_save_data_path` replaces it. `EngineBuilder::resolve` calls `enter_bundle_dir()` so release bundles run from their own folder. Embedded keys `RYO_EMBED_PORTABLE`/`RYO_EMBED_SAVE_FOLDER` feed it.

In release builds `Workspace::discover()` builds its config from embedded values (`Workspace::from_embedded`: `[system]`, `[debug]`, title, `[release]`, and `[env]` via the `RYO_EMBED_ENV` JSON) and never reads a Ryo.toml on disk, so `ryo2d run` matches a shipped game. `[tilemap] tiled_project` (`WorkspaceConfig::tiled_project`) ships as `RYO_EMBED_TILED_PROJECT`, empty for none.

## Gotchas

- Two `DebugConfig` structs: `workspace.rs` (TOML parsing) and `debug.rs` (runtime). A new debug flag needs both plus fields in `resolve.rs` (`ProjectLayer`, `ResolvedSettings`, and `BuilderOverrides` if it has a setter), the copy in `EngineBuilder::resolve_in`, and an `RYO_EMBED_*` key
- `system.json` holds player preferences - don't commit it
- Frame timings: `Engine::frame` measures update, render and present (`FrameTimings`; present leaves out the renderer's wait for the window's next image, `WgpuRenderer::acquire_wait`, kept as `wait`), and at its end (`publish_frame_cost`) folds them into `SmoothedFrameTimings` (a plain mean until 50 frames, then an exponential average) and puts `FrameCost` (prelude, read-only accessors) in the world, windowed or headless. The F1 overlay draws the same smoothed values; the slow-frame warning in `after_frame` measures from the frame's start with waits and dev windows included
