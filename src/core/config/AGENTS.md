# AGENTS.md - Config Module

See skill `ryo-config-save` for Ryo.toml layout and the save API.

## Configuration Precedence

`EngineBuilder::resolve_in` in `engine.rs` calls `resolve::resolve_settings` (pure, unit-tested). Lowest to highest:
1. Engine defaults (`ResolvedSettings::defaults`)
2. Project values: Ryo.toml (dev) or `RYO_EMBED_*` values from `ryo2d build` (release) - `ProjectLayer`
3. Builder calls - `BuilderOverrides`
4. `system.json` preferences - only with `.with_user_preferences(filter)`, only keys present in the file and in the (combined) filter - `StoredPreferences`

- A missing system.json key is "no preference", not a default
- A new system.json is seeded from levels 1-3; headless builds never read or write it
- After the first run, `data/system.json` holds the filtered settings and overrides later builder or `[system]` changes - delete it in dev to re-seed.
- Embedded keys live in `resolve::COMPILED_EMBEDDED`, `cli/commands/build.rs::embedded_config_env` and `src/build.rs` (tests keep them in sync)
- Filter with `UserPreference` / `UserPreferenceFilter` (`all()`, `none()`, or an array of fields):
  - Fixed-resolution games: leave out `Resolution`
  - Audio-only settings: volume fields only

## Save Location

`save_location.rs` (pure, unit-tested) decides release paths (`valid_save_folder`, `release_save_dir`, `compiled_release_save_dir`, `bundle_dir_to_enter`, `enter_bundle_dir`). `Workspace::discover()` applies the release save dir (OS user folder + `[release] save_folder`/`[project] name`, or `data/` beside the game when portable); `from_path`/`discover_from` stay dev-style; `set_save_data_path` replaces it. `EngineBuilder::resolve` calls `enter_bundle_dir()` so release bundles run from their own folder. Embedded keys `RYO_EMBED_PORTABLE`/`RYO_EMBED_SAVE_FOLDER` feed it.

In release builds `Workspace::discover()` builds its config from embedded values (`Workspace::from_embedded`: `[system]`, `[debug]`, title, `[release]`, and `[env]` via the `RYO_EMBED_ENV` JSON) and never reads a Ryo.toml on disk, so `ryo2d run` matches a shipped game.

## Gotchas

- Two `DebugConfig` structs: `workspace.rs` (TOML parsing) and `debug.rs` (runtime). A new debug flag needs both plus fields in `resolve.rs` (`ProjectLayer`, `ResolvedSettings`, and `BuilderOverrides` if it has a setter), the copy in `EngineBuilder::resolve_in`, and an `RYO_EMBED_*` key
- `system.json` holds player preferences - don't commit it
