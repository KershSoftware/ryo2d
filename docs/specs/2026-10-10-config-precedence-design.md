# Config Precedence - Design

## Overview

Make every startup setting follow one documented rule, so builder calls, `Ryo.toml`, `ryo2d build` embedded values and saved user preferences (`system.json`) combine predictably in dev and in shipped builds.

**The rule:** engine defaults → embedded values (release) *or* `Ryo.toml` (dev) → builder calls → saved user preferences. Each layer only overrides what it actually sets; user preferences only override keys that are both **present in `system.json`** and **in the `with_user_preferences` filter**.

**Why:** an investigation (2026-10-10) found the rule broken field by field, all accidentally:
- builder `.show_colliders/.show_velocities/.show_origins` are clobbered in dev and release; `.background_fps/.background_rendering` are clobbered in dev only;
- keys missing from `system.json` (e.g. written under a narrower filter) load as hardcoded defaults that beat builder and `Ryo.toml`; a missing `vsync` erases a builder `.vsync()`; `save_system` never writes missing keys back, so in-game changes to them are lost;
- the "one-time" resolution migration resets a saved 1280×720 to `Ryo.toml`'s value on every launch;
- new `system.json` files are seeded from `Ryo.toml` — not shipped, so shipped games seed engine defaults that then beat embedded and builder values forever;
- `disable_debug_keys`, `dev_window_keys`, `logger_window`, `enable_profiler`, `background_fps/rendering` are never embedded, so shipped games keep F1-F4 debug overlays live even when `Ryo.toml` disables them;
- volume defaults disagree in three places (1.0/0.8/1.0, 0.8/1.0/1.0, 0.7/1.0/1.0);
- ~30 doc statements describe other precedence orders or removed APIs.

This bug class has been patched field by field four times (233458a, 83d9f26, 9c6203b, 4770698); this design replaces the per-field flags with one layering mechanism.

**Out of scope:** builder-only settings (`target_fps`, `physics_hz`, `window_mode`, `max_width/height`, `ui_scale`) keep their current behaviour; new mdbook content (mdbook is being retired — only minimal corrections).

## Layered Resolution

New module `src/core/config/resolve.rs` holds the pure resolution logic (no I/O), keeping it out of the 4,700-line `engine.rs`.

```rust
/// Settings the builder may override; `None` = not set by the builder.
pub(crate) struct BuilderOverrides {
  pub resolution: Option<(u32, u32)>,
  pub display_mode: Option<DisplayMode>,
  pub vsync: Option<bool>,
  pub title: Option<String>,
  pub show_fps: Option<bool>,
  pub show_colliders: Option<bool>,
  pub show_velocities: Option<bool>,
  pub show_origins: Option<bool>,
  pub background_fps: Option<u32>,
  pub background_rendering: Option<bool>,
  pub icon: Option<PathBuf>,
}

/// Values from `Ryo.toml` (dev) or from `ryo2d build` embedding (release).
pub(crate) struct ProjectLayer { /* same fields as the resolved output, each Option<_> */ }

/// User preferences actually present in system.json (`None` = key absent).
pub(crate) struct StoredPreferences {
  pub resolution: Option<(u32, u32)>,
  pub display_mode: Option<DisplayMode>,
  pub vsync: Option<bool>,
  pub master_volume: Option<f32>,
  pub music_volume: Option<f32>,
  pub sfx_volume: Option<f32>,
  pub show_fps: Option<bool>,
}

pub(crate) fn resolve_settings(
  project: &ProjectLayer,
  builder: &BuilderOverrides,
  prefs: Option<(&StoredPreferences, &UserPreferenceFilter)>,
) -> ResolvedSettings;
```

Volumes have no builder setter (today or after this change), so `BuilderOverrides` has no volume fields.

`ResolvedSettings` carries every field above plus the three volumes (non-optional) plus the debug-only fields `show_frame_timing`, `enable_profiler`, `disable_debug_keys`, `logger_window`, `dev_window_keys`, `dev_console` (project layer only — they have no builder setter today) and `game_name` / `game_version`.

**Order per field:** default → project layer (if `Some`) → builder (if `Some`) → preference (if in filter and `Some`). Defaults come from one place: `SystemDefaults` / `DebugConfig` defaults, with volumes unified at **master 0.8, music 1.0, sfx 1.0** (the documented `Ryo.toml` default; the builder's 1.0/0.8/1.0 and the embedded parse fallback 0.7 go away).

`EngineBuilder` stores a `BuilderOverrides` instead of writing into `EngineConfig` and setting flags. Removed: `resolution_set`, `display_mode_set`, `show_fps_set`, the six `*_user_pref` flags, the dead setter guards, the `"Ryo2D"` title sentinel, and the tests that poke those flags. The `RYO_PROFILER_AUTO_OPEN` override and the release `icon.png`-next-to-exe lookup stay, applied after layering as today.

`EngineBuilder::resolve(self, headless)` becomes a thin wrapper over `resolve_in(self, workspace: Option<&Workspace>, headless)`, which builds the `ProjectLayer` (embedded if present, else from the workspace), loads `StoredPreferences` (skipped when headless or when no filter was given), calls `resolve_settings`, and assembles `ResolvedBuild`. Tests call `resolve_in` with a temp-dir `Workspace` (`Workspace::from_path`), never touching the real save directory.

## system.json

- **Loading:** the preference loader reports which keys are present (`StoredPreferences`), instead of filling absent keys with hardcoded serde defaults. Only present ∩ filter keys override.
- **Seeding:** when `system.json` doesn't exist, it is created from the settings resolved *without* preferences (defaults → project → builder), for the fields in the filter. Shipped games therefore seed embedded/builder values, not engine defaults.
- **Migration deleted:** the resolution rewrite in `merge_defaults` (save.rs ~595-605) is removed; the tests that assert it (save.rs ~1622, ~1730) are inverted to assert a saved 1280×720 survives.
- **Saving:** `save_system` writes a key that is missing from the file when its value changes (the existing "new flat keys" loop in save.rs ~875-885 can never fire today).
- **Public API:** `SystemState` stays fully populated for game menus (from the resolved settings); `SaveManager` public methods keep their signatures.

## Release Parity

`ryo2d build` (`src/cli/commands/build.rs`) additionally embeds `RYO_EMBED_DISABLE_DEBUG_KEYS`, `RYO_EMBED_ENABLE_PROFILER`, `RYO_EMBED_DEV_WINDOW_KEYS`, `RYO_EMBED_LOGGER_WINDOW`, `RYO_EMBED_BACKGROUND_FPS` and `RYO_EMBED_BACKGROUND_RENDERING` from `Ryo.toml`, and the project layer reads them. `src/build.rs`'s `rerun-if-env-changed` list is updated to the current variable names (it still lists pre-83d9f26 names).

## Error Handling

Unchanged externally: malformed embedded values fall back to defaults with the existing warning; a malformed or unreadable `system.json` is treated as "no stored preferences" with a warning, and is not overwritten until the game saves system settings.

## Testing

**Unit (pure, `resolve.rs`):** table-driven tests per field — default only; project overrides default; builder overrides project; preference overrides builder; preference outside the filter is ignored; preference key absent falls through to builder/project. Includes the debug flags (`show_colliders` survives the project layer; `background_fps` likewise).

**Integration (temp-dir workspace via `resolve_in`):**
- a `system.json` written under a narrower filter: builder resolution/display mode/vsync and the `Ryo.toml` master volume win for the absent keys;
- a saved 1280×720 is not migrated when `Ryo.toml` says 1920×1080;
- first-run seeding writes the builder/project values, not engine defaults;
- `save_system` persists a changed value for a key that was missing from the file;
- headless builds never create or read `system.json` (existing e2e test keeps passing).

**CLI:** a unit test that the build command's embed list includes the six new variables (assert on the function that builds the env map, if one exists; otherwise extract one).

## Documentation

Correct every stale statement found in the investigation: `engine.rs` builder rustdoc (`with_user_preferences`, `vsync`, chain comments), `save.rs` module/struct/method docs, `workspace.rs`, `scene.rs:225`, `config/debug.rs` (dev keys in release), `src/core/config/AGENTS.md`, and the shipped `ryo-config-save`, `ryo-game-dev` and `ryo-api` skills (both copies each, CRLF preserved). In `docs/src/concepts/system-configuration.md` and `docs/src/getting-started/configuration.md`, replace the wrong precedence chain and removed API names (`with_system_state_overrides`, `SystemField*`) with the rule above — no new sections.

## Design Decisions

- **Generic layering over per-field flags:** one mechanism, testable as a pure function; ends the field-by-field patch cycle.
- **Missing key = no preference:** a preference is something the player chose; absent keys must not masquerade as choices.
- **Seed from resolved values:** the file should start from what the game would actually use, which in shipped builds is embedded + builder values.
- **Delete the migration:** it targeted files from before `[system]` existed (January 2026) and has been silently overriding saved resolutions since.
- **Embed the dev-only flags:** a shipped game should behave like its dev build, especially `disable_debug_keys`.
