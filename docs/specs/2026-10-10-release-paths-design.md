# Release Paths - Design

## Overview

Make shipped games (binaries made by `ryo2d build`) find their files wherever they're launched from, and keep player data in the OS per-user data folder by default.

**Why:** a release bundle (`release/`: the game binary, `assets/`, `levels/`, `data/`) resolves every path against the process working directory. `ryo2d run` and a Windows double-click set that to the bundle, but macOS Finder and many Linux file managers launch from the home folder, so assets fail to load and saves land in (or fail to create) `~/data`. Installed games also can't save beside the binary when its folder is read-only (`Program Files`, a signed macOS app).

**Owner decisions (2026-10-10):**
- Shipped games save to the OS user folder by default; `[release] portable = true` keeps `data/` beside the game.
- The per-game folder is `[release] save_folder` when set (e.g. `"MyStudio/MyGame"`), otherwise `[project] name`.
- No migration: players of already-shipped games start fresh in the OS folder.

**Out of scope:** dev builds (`cargo run`, `ryo2d dev`) and headless runs — their behaviour is unchanged; migrating existing player data; `[env]` in release builds; per-platform app bundles (`.app`) and installers.

## Behaviour

**Release detection.** A binary is a release build when `RYO_EMBED_RESOLUTION` was embedded (`resolve::compiled_embedded_value`), as the config resolver already uses.

**Launch folder.** At startup (`EngineBuilder::resolve`, non-headless), a release build whose executable folder looks like a bundle — it contains an `assets/`, `levels/` or `data/` directory — and differs from the working directory switches the working directory to the executable folder. Failure to switch logs a warning and continues. Running a release binary straight from `target/release` (no bundle folders there) and dev builds are untouched.

**Save location** (what `Workspace::save_data_dir` returns and creates):

| Build | Setting | Save directory |
|---|---|---|
| dev | — | `<workspace root>/<[paths] save_data>` (unchanged) |
| release | default | `<OS data dir>/<save folder>` — `dirs::data_dir()`: `%APPDATA%`, `~/Library/Application Support`, `~/.local/share` |
| release | `[release] portable = true` | `<executable folder>/data` |
| release | no OS data dir, or no/invalid embedded save folder | `<executable folder>/data`, with a warning |

`Workspace::discover()` applies the release save directory; `Workspace::from_path` / `discover_from` (used by tools and tests) stay dev-style. `Workspace::set_save_data_path` (an explicit runtime choice by the game) replaces it. A new `Workspace::save_location()` returns the directory without creating it. The engine logs the save location once at startup (non-headless) so players and developers can find it.

**Save folder name.** Valid names are non-empty relative paths made only of normal components (no `..`, `.`, root or drive), with no `<>:"|?*\` or control characters and no component ending in `.` or a space (Windows). `ryo2d build` fails with a clear message when the resolved name (`save_folder` or `[project] name`) is invalid; the engine re-checks at runtime and falls back to `data/` beside the game with a warning.

**Embedding.** Two new embedded keys, `RYO_EMBED_PORTABLE` and `RYO_EMBED_SAVE_FOLDER` (the resolved name), join `resolve::COMPILED_EMBEDDED`, `cli::commands::build::embedded_config_env` and `src/build.rs`; the existing sync tests cover them. They are not layered settings (`ProjectLayer` ignores them).

**Ryo.toml.** `[release]` gains `portable` (bool, default false) and `save_folder` (string, optional); `ReleaseConfig` gains `portable: bool` and `save_folder: Option<String>`.

**Bundle.** `ryo2d build` creates `release/data/` only in portable mode. `ryo2d run` launches the bundle like a player would, so a non-portable game saves into the developer's real OS folder for that game.

## Error Handling

- No OS data dir, invalid embedded folder, or missing embedded folder (engine built by an older CLI): fall back to `<executable folder>/data`, warn once.
- Working-directory switch fails: warn, keep going (paths behave as today).
- Invalid `save_folder` / `[project] name` at build time: `ryo2d build` stops before compiling with `CliError::BuildFailed` naming the value and the rule.
- No `unwrap`/`expect` in library code.

## Testing

**Unit (pure functions, `config/save_location.rs`):** `valid_save_folder` accepts/rejects a table of names; `release_save_dir` covers not-a-release, OS folder, portable, no OS dir, invalid and missing folder; `bundle_dir_to_enter` covers dev, same folder, bundle folder, and a `target/release`-like folder.

**Workspace:** `[release] portable`/`save_folder` parse; dev `save_location` is unchanged; `set_save_data_path` replaces the release override.

**CLI:** the two keys are embedded (default folder = `[project] name`, override wins, portable flag); invalid names are rejected before building.

**End to end:** compiling with `RYO_EMBED_RESOLUTION` and `RYO_EMBED_SAVE_FOLDER` set makes the dev-location workspace test fail (the save location moves into the OS folder), then passes again when unset. Controller check: build an example with `ryo2d build`, launch the bundle binary from another directory with `RYO_CAPTURE_FRAMES`/`RYO_CAPTURE_EXIT`, and confirm it rendered (assets loaded) and wrote `system.json` under the OS folder.

## Documentation

`ryo-config-save` and `ryo-cli` skills (both copies, CRLF), `src/core/config/AGENTS.md`, `src/cli/AGENTS.md`, rustdoc on `Workspace::save_data_dir` / `save_location` / `create_save_manager` / `discover`, and commented `[release]` options in the `ryo2d init` template. No mdbook pages.

## Design Decisions

- **Resolve inside `Workspace`:** games already build save paths through `Workspace::discover().create_save_manager()` (17 call sites in the examples) and the engine loads preferences the same way, so every path picks up the new location without game changes.
- **OS folder by default, portable opt-in:** the platform norm for installed games, and it works in read-only install locations; portable keeps zip-and-go builds possible.
- **Switch the working directory once, at engine build:** relative asset paths are used across many APIs (`AssetManager`, audio, themes, levels); one switch fixes all of them, and only for release bundles.
- **Bundle detection by its folders:** distinguishes a bundle from `target/release`, so `ryo2d run`'s fallback (running the binary in `target/release` from the project root) keeps working.
