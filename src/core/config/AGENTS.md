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

## Gotchas

- Two `DebugConfig` structs: `workspace.rs` (TOML parsing) and `debug.rs` (runtime). A new debug flag needs both plus fields in `resolve.rs` (`ProjectLayer`, `ResolvedSettings`, and `BuilderOverrides` if it has a setter), the copy in `EngineBuilder::resolve_in`, and an `RYO_EMBED_*` key
- `system.json` holds player preferences - don't commit it
