# AGENTS.md - Config Module

See skill `ryo-config-save` for Ryo.toml layout and the save API.

## Configuration Precedence

Highest to lowest (`EngineBuilder::build()` in `engine.rs`):
1. `system.json` user preferences - only if `.with_user_preferences(filter)` is called, only filtered fields
2. Explicit builder calls (`.resolution()`, `.vsync()`, `.display_mode()`, ...)
3. Values embedded at compile time by `ryo2d build` (replace runtime Ryo.toml lookup)
4. Ryo.toml `[system]` / `[debug]` (runtime workspace discovery)
5. Engine defaults

- Without `with_user_preferences()`, system.json is never read
- User prefs beat builder calls in either call order - e.g. `.resolution()` is a no-op once the `Resolution` pref is loaded
- Filter with `UserPreference` / `UserPreferenceFilter` (`all()`, `none()`, or an array of fields):
  - Fixed-resolution games: leave out `Resolution`
  - Audio-only settings: volume fields only

## Gotchas

- Two `DebugConfig` structs: `workspace.rs` (TOML parsing) and `debug.rs` (runtime). A new debug flag needs both plus the field copy in `EngineBuilder::build()`
- `system.json` holds player preferences - don't commit it
