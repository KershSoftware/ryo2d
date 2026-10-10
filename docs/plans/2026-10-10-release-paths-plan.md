# Release Paths Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Binaries made by `ryo2d build` find their assets wherever they're launched from, and keep player data in the OS per-user data folder by default (or `data/` beside the game when `[release] portable = true`).

**Architecture:** A new pure module `src/core/config/save_location.rs` decides the save folder name rules, the release save directory and whether to enter the bundle folder. `Workspace::discover()` applies the release save directory (so every `Workspace::discover().create_save_manager()` call and the engine's preference loading follow it); `EngineBuilder::resolve` switches a release build into its bundle folder before anything resolves paths. Two new embedded keys carry `[release] portable` and the save folder name from Ryo.toml into the binary.

**Tech Stack:** Rust 2024, `dirs` 6 (already in the lock file), tempfile (dev-dependency).

**Spec:** `docs/plans/2026-10-10-release-paths-design.md`

## Global Constraints

- Rust 2024 edition, 2-space indentation, no `unwrap`/`expect` in library code (tests may use them).
- Format only files you change, with `rustfmt --edition 2024 <file>`; never plain rustfmt on `src/lib.rs`, `src/core/mod.rs`, `src/core/config/mod.rs` or `src/cli/commands/mod.rs` (they recurse into submodules) — use `--check` on those and fix only lines you wrote.
- CRLF files stay CRLF on every line: the skill files under `src/cli/skills/` and `.claude/skills/`. Edit them byte-preservingly; check with `file`.
- Warnings use `eprintln!("[ryo2d] Warning: ...")`.
- Dev builds (`cargo run`, `ryo2d dev`) and headless runs keep today's behaviour: save under the workspace root, never change the working directory.
- Embedded keys stay in sync across `resolve::COMPILED_EMBEDDED`, `cli/commands/build.rs::embedded_config_env` and `src/build.rs` (existing tests enforce it).
- Commit on `main`, one commit per task, with the message given in the task. No co-author lines. Do not push.
- No mdbook pages.
- Done means: `cargo test --workspace` 0 failed and `cargo clippy --workspace --all-targets` with no new warnings in touched files.

## Review Focus

1. Save folder names with spaces or non-ASCII letters (`"Star Hopper"`, `"ゲーム"`) must be accepted — Task 1 `valid_save_folder_accepts_and_rejects`.
2. An engine built by an older CLI (no `RYO_EMBED_SAVE_FOLDER`) must still save somewhere usable — Task 2 `missing_save_folder_falls_back_beside_the_game`.
3. A game that sets its save path at runtime (`set_save_data_path`) gets exactly that path in release — Task 2 `set_save_data_path_replaces_the_release_location`.
4. `ryo2d run`'s fallback (a release binary in `target/release`, launched from the project root) must not switch folders — Task 3 `target_release_folders_are_not_bundles`.
5. Fallback warnings must print once per process, not on every `Workspace::discover()` call (games call it from many scenes) — Task 2 caches the compiled result in a `OnceLock`; the reviewer checks it.

---

### Task 1: `[release] portable` and `save_folder` reach release builds

**Files:**
- Create: `src/core/config/save_location.rs`
- Modify: `src/core/config/mod.rs` (add `pub(crate) mod save_location;` in alphabetical order)
- Modify: `src/core/config/workspace.rs` (`ReleaseConfig` ~178, `ReleaseSection` ~283, mapping ~542, tests)
- Modify: `src/core/config/resolve.rs` (`COMPILED_EMBEDDED` ~312, test `compiled_table_has_unique_embed_keys` ~647)
- Modify: `src/cli/commands/build.rs` (`execute`, `embedded_config_env`, tests)
- Modify: `src/build.rs` (`EMBEDDED_KEYS`)

**Interfaces:**
- Produces: `pub(crate) fn save_location::valid_save_folder(name: &str) -> bool`; `ReleaseConfig { logo, pub portable: bool, pub save_folder: Option<String> }`; embedded keys `RYO_EMBED_PORTABLE` (`"true"`/`"false"`) and `RYO_EMBED_SAVE_FOLDER` (the resolved, validated name).

- [ ] **Step 1: Write the failing tests**

Create `src/core/config/save_location.rs` with only its doc and tests for now:

```rust
//! Where release builds (`ryo2d build`) keep save data, and which folder they run from.

use std::path::{Component, Path};

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn valid_save_folder_accepts_and_rejects() {
    for name in ["MyGame", "Star Hopper", "MyStudio/MyGame", "ゲーム", "game-2_final"] {
      assert!(valid_save_folder(name), "{name:?} should be accepted");
    }
    for name in [
      "", "   ", "/abs", "../up", "a/../b", ".", "C:\\Games", "a:b", "bad|name", "trailing.",
      "trailing ", "tab\tname",
    ] {
      assert!(!valid_save_folder(name), "{name:?} should be rejected");
    }
  }
}
```

In `src/core/config/mod.rs` add `pub(crate) mod save_location;` between `pub mod save;` and `pub mod workspace;` (check with `rustfmt --edition 2024 --check src/core/config/mod.rs`).

In `workspace.rs`'s `tests` module add:

```rust
  #[test]
  fn release_section_reads_portable_and_save_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
      dir.path().join("Ryo.toml"),
      "[release]\nportable = true\nsave_folder = \"Studio/Game\"\n",
    )
    .unwrap();
    let ws = Workspace::from_path(dir.path()).unwrap();
    assert!(ws.config().release.portable);
    assert_eq!(ws.config().release.save_folder.as_deref(), Some("Studio/Game"));

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ryo.toml"), "").unwrap();
    let ws = Workspace::from_path(dir.path()).unwrap();
    assert!(!ws.config().release.portable);
    assert_eq!(ws.config().release.save_folder, None);
  }
```

In `src/cli/commands/build.rs`'s `tests` module add:

```rust
  fn embedded_value(env: &[(&'static str, String)], key: &str) -> String {
    env
      .iter()
      .find(|(name, _)| *name == key)
      .map(|(_, value)| value.clone())
      .unwrap()
  }

  #[test]
  fn save_folder_defaults_to_the_project_name() {
    let mut config = WorkspaceConfig::default();
    config.name = "Star Hopper".to_string();
    let env = embedded_config_env(&config, None);
    assert_eq!(embedded_value(&env, "RYO_EMBED_SAVE_FOLDER"), "Star Hopper");
    assert_eq!(embedded_value(&env, "RYO_EMBED_PORTABLE"), "false");
  }

  #[test]
  fn save_folder_override_and_portable_are_embedded() {
    let mut config = WorkspaceConfig::default();
    config.release.save_folder = Some("Studio/Hopper".to_string());
    config.release.portable = true;
    let env = embedded_config_env(&config, None);
    assert_eq!(embedded_value(&env, "RYO_EMBED_SAVE_FOLDER"), "Studio/Hopper");
    assert_eq!(embedded_value(&env, "RYO_EMBED_PORTABLE"), "true");
  }

  #[test]
  fn invalid_save_folder_stops_the_build() {
    let mut config = WorkspaceConfig::default();
    config.release.save_folder = Some("../escape".to_string());
    assert!(matches!(checked_save_folder(&config), Err(CliError::BuildFailed(_))));

    config.release.save_folder = None;
    config.name = "Hopper".to_string();
    assert!(matches!(checked_save_folder(&config), Ok(ref folder) if folder == "Hopper"));
  }
```

In `resolve.rs`, change `compiled_table_has_unique_embed_keys`'s `assert_eq!(keys.len(), 19);` to `assert_eq!(keys.len(), 21);`.

- [ ] **Step 2: Run the tests to watch them fail**

Run: `cargo test --lib -- config::save_location config::workspace cli::commands::build config::resolve 2>&1 | tail -20`
Expected: compile errors — `valid_save_folder`, `ReleaseConfig::portable`/`save_folder`, `checked_save_folder` don't exist.

- [ ] **Step 3: Implement**

In `save_location.rs`, above the tests:

```rust
/// Whether `name` can name a per-game folder under the OS user data directory.
///
/// Valid names are non-empty relative paths of normal components (e.g.
/// `"MyGame"` or `"MyStudio/MyGame"`), without characters Windows rejects and
/// without components ending in `.` or a space.
pub(crate) fn valid_save_folder(name: &str) -> bool {
  const FORBIDDEN: &[char] = &['<', '>', ':', '"', '|', '?', '*', '\\'];
  if name.trim().is_empty() || name.chars().any(|c| c.is_control() || FORBIDDEN.contains(&c)) {
    return false;
  }
  Path::new(name).components().all(|component| match component {
    Component::Normal(part) => part
      .to_str()
      .is_some_and(|part| !part.ends_with('.') && !part.ends_with(' ')),
    _ => false,
  })
}
```

In `workspace.rs`:

```rust
pub struct ReleaseConfig {
  /// Path to custom application icon (PNG), relative to workspace root.
  /// If None, the Ryo2D default logo is used.
  pub logo: Option<PathBuf>,
  /// Keep save data in `data/` beside the game instead of the OS user data
  /// folder (`[release] portable`, default `false`).
  pub portable: bool,
  /// Folder under the OS user data folder for this game's saves
  /// (`[release] save_folder`); `None` uses `[project] name`.
  pub save_folder: Option<String>,
}
```

`ReleaseSection` gains `portable: Option<bool>` and `save_folder: Option<String>` (with one-line docs), and the mapping becomes:

```rust
    let release = ReleaseConfig {
      logo: release_section.logo.map(PathBuf::from),
      portable: release_section.portable.unwrap_or(false),
      save_folder: release_section.save_folder,
    };
```

In `resolve.rs`, append `"RYO_EMBED_PORTABLE",` and `"RYO_EMBED_SAVE_FOLDER",` to the `compiled_embedded![...]` list, and add one sentence to the table's doc: "`RYO_EMBED_PORTABLE` and `RYO_EMBED_SAVE_FOLDER` are read by `save_location`, not the settings layers."

In `src/cli/commands/build.rs`, add:

```rust
/// The per-game save folder name: `[release] save_folder`, else `[project] name`.
fn release_save_folder(config: &crate::core::config::WorkspaceConfig) -> String {
  config
    .release
    .save_folder
    .clone()
    .unwrap_or_else(|| config.name.clone())
}

/// [`release_save_folder`], rejected before building when it can't name a folder.
fn checked_save_folder(config: &crate::core::config::WorkspaceConfig) -> CliResult<String> {
  let folder = release_save_folder(config);
  if crate::core::config::save_location::valid_save_folder(&folder) {
    Ok(folder)
  } else {
    Err(CliError::BuildFailed(format!(
      "Invalid save folder \"{folder}\" (from [release] save_folder or [project] name). \
       Use a relative name like \"MyStudio/MyGame\" without \"..\", a drive or any of <>:\"|?*\\"
    )))
  }
}
```

Append to the `vec![...]` in `embedded_config_env`:

```rust
    ("RYO_EMBED_PORTABLE", config.release.portable.to_string()),
    ("RYO_EMBED_SAVE_FOLDER", release_save_folder(config)),
```

In `execute`, right after `let release = &config.release;`, add:

```rust
  // Fail before compiling if the save folder name can't be used on every OS
  checked_save_folder(config)?;
```

In `src/build.rs`, append `"RYO_EMBED_PORTABLE",` and `"RYO_EMBED_SAVE_FOLDER",` to `EMBEDDED_KEYS`.

Fix any other compile errors from the new `ReleaseConfig` fields with the smallest change (none are expected; `grep -rn "ReleaseConfig {" src tests examples`).

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib -- config:: cli::commands::build 2>&1 | grep -E "test result|FAILED"`
Expected: all pass, including `embedded_env_covers_every_key_the_engine_reads` and `build_script_tracks_every_embedded_key`.

- [ ] **Step 5: Full suite, lint, commit**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head` and `cargo clippy --workspace --all-targets 2>&1 | grep -E "^\s+--> src/(core/config/(save_location|workspace|resolve)|cli/commands/build|build)\.rs"`
Expected: 0 failed; no clippy findings in those files except dead-code for `valid_save_folder` users that arrive in Task 2 (none expected — the CLI uses it).

```bash
git add src/core/config/save_location.rs src/core/config/mod.rs src/core/config/workspace.rs src/core/config/resolve.rs src/cli/commands/build.rs src/build.rs
git commit -m "feat(config): add [release] portable and save_folder settings"
```

---

### Task 2: Release builds save to the OS user folder

**Files:**
- Modify: `Cargo.toml` (`[dependencies]`: `dirs = "6"`)
- Modify: `src/core/config/save_location.rs`
- Modify: `src/core/config/workspace.rs` (`Workspace` struct ~88, constructors, `discover` ~317, `save_data_dir` ~608, `set_save_data_path` ~636, tests)
- Modify: `src/core/engine.rs` (`Engine::new`, before the hot-reload block ~621)

**Interfaces:**
- Consumes (Task 1): `valid_save_folder`, embedded keys `RYO_EMBED_PORTABLE`, `RYO_EMBED_SAVE_FOLDER`; `resolve::compiled_embedded_value`.
- Produces: `pub(crate) fn release_save_dir<'a>(lookup: impl Fn(&str) -> Option<&'a str>, exe_dir: &Path, user_data_dir: Option<PathBuf>) -> Option<PathBuf>`; `pub(crate) fn compiled_release_save_dir() -> Option<PathBuf>`; `pub fn Workspace::save_location(&self) -> PathBuf`.

- [ ] **Step 1: Write the failing tests**

In `save_location.rs`'s tests module add:

```rust
  fn embedded(
    values: &'static [(&'static str, &'static str)],
  ) -> impl Fn(&str) -> Option<&'static str> {
    move |name| values.iter().find(|(key, _)| *key == name).map(|(_, value)| *value)
  }

  fn game_dir() -> PathBuf {
    PathBuf::from("/games/hopper")
  }

  fn os_data() -> Option<PathBuf> {
    Some(PathBuf::from("/users/me/data"))
  }

  #[test]
  fn dev_builds_have_no_release_save_dir() {
    assert_eq!(release_save_dir(|_| None, &game_dir(), os_data()), None);
  }

  #[test]
  fn release_builds_save_in_the_os_user_folder() {
    let lookup = embedded(&[
      ("RYO_EMBED_RESOLUTION", "1280x720"),
      ("RYO_EMBED_PORTABLE", "false"),
      ("RYO_EMBED_SAVE_FOLDER", "Studio/Hopper"),
    ]);
    assert_eq!(
      release_save_dir(lookup, &game_dir(), os_data()),
      Some(PathBuf::from("/users/me/data").join("Studio/Hopper"))
    );
  }

  #[test]
  fn portable_release_builds_save_beside_the_game() {
    let lookup = embedded(&[
      ("RYO_EMBED_RESOLUTION", "1280x720"),
      ("RYO_EMBED_PORTABLE", "true"),
      ("RYO_EMBED_SAVE_FOLDER", "Hopper"),
    ]);
    assert_eq!(release_save_dir(lookup, &game_dir(), os_data()), Some(game_dir().join("data")));
  }

  #[test]
  fn no_os_data_dir_falls_back_beside_the_game() {
    let lookup = embedded(&[("RYO_EMBED_RESOLUTION", "1280x720"), ("RYO_EMBED_SAVE_FOLDER", "Hopper")]);
    assert_eq!(release_save_dir(lookup, &game_dir(), None), Some(game_dir().join("data")));
  }

  #[test]
  fn missing_save_folder_falls_back_beside_the_game() {
    let lookup = embedded(&[("RYO_EMBED_RESOLUTION", "1280x720")]);
    assert_eq!(release_save_dir(lookup, &game_dir(), os_data()), Some(game_dir().join("data")));
  }

  #[test]
  fn invalid_save_folder_falls_back_beside_the_game() {
    let lookup = embedded(&[("RYO_EMBED_RESOLUTION", "1280x720"), ("RYO_EMBED_SAVE_FOLDER", "../escape")]);
    assert_eq!(release_save_dir(lookup, &game_dir(), os_data()), Some(game_dir().join("data")));
  }
```

(Add `use std::path::PathBuf;` to the module's imports if it isn't there.)

In `workspace.rs`'s tests module add:

```rust
  #[test]
  fn dev_save_location_is_inside_the_workspace() {
    let ws = Workspace::discover().unwrap();
    assert!(ws.save_location().starts_with(ws.root()));
  }

  #[test]
  fn set_save_data_path_replaces_the_release_location() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::from_path(dir.path()).unwrap();
    ws.release_save_dir = Some(PathBuf::from("/users/me/data/Hopper"));
    assert_eq!(ws.save_location(), PathBuf::from("/users/me/data/Hopper"));

    ws.set_save_data_path("saves");

    assert_eq!(ws.save_location(), dir.path().join("saves"));
  }
```

- [ ] **Step 2: Run the tests to watch them fail**

Run: `cargo test --lib -- config::save_location config::workspace 2>&1 | tail -15`
Expected: compile errors — `release_save_dir`, `Workspace::save_location`, `Workspace::release_save_dir` don't exist.

- [ ] **Step 3: Implement**

`Cargo.toml` `[dependencies]`: add `dirs = "6"` (alphabetical position among the existing entries).

In `save_location.rs` (imports become `use std::path::{Component, Path, PathBuf}; use std::sync::OnceLock; use super::resolve::compiled_embedded_value;`):

```rust
/// The save directory of a release build, or `None` for a dev build.
///
/// `lookup` reads embedded values. Portable builds keep saves in `data/` beside
/// the executable; so do builds whose OS data directory or embedded save
/// folder is unusable, with a warning.
pub(crate) fn release_save_dir<'a>(
  lookup: impl Fn(&str) -> Option<&'a str>,
  exe_dir: &Path,
  user_data_dir: Option<PathBuf>,
) -> Option<PathBuf> {
  lookup("RYO_EMBED_RESOLUTION")?;
  let beside_game = exe_dir.join("data");
  if lookup("RYO_EMBED_PORTABLE") == Some("true") {
    return Some(beside_game);
  }
  let Some(folder) = lookup("RYO_EMBED_SAVE_FOLDER").filter(|folder| valid_save_folder(folder))
  else {
    eprintln!(
      "[ryo2d] Warning: No valid save folder was embedded, saving to {}",
      beside_game.display()
    );
    return Some(beside_game);
  };
  match user_data_dir {
    Some(base) => Some(base.join(folder)),
    None => {
      eprintln!(
        "[ryo2d] Warning: No user data folder on this system, saving to {}",
        beside_game.display()
      );
      Some(beside_game)
    }
  }
}

/// [`release_save_dir`] for this binary, computed once per process.
pub(crate) fn compiled_release_save_dir() -> Option<PathBuf> {
  static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
  DIR
    .get_or_init(|| {
      compiled_embedded_value("RYO_EMBED_RESOLUTION")?;
      let exe = std::env::current_exe().ok()?;
      release_save_dir(compiled_embedded_value, exe.parent()?, dirs::data_dir())
    })
    .clone()
}
```

In `workspace.rs`:
- `Workspace` gains `/// Save directory of a release build (\`ryo2d build\`); \`None\` in dev` `release_save_dir: Option<PathBuf>,`; both constructors set it to `None`.
- `discover`:

```rust
  /// Discover workspace by searching for `ryo.toml` from current directory.
  ///
  /// Searches the current directory and up to 3 parent directories.
  /// If no config file is found, uses the current directory as workspace root
  /// with default settings. In a release build made by `ryo2d build`, save
  /// data goes to the OS user data folder (see [`save_location`](Self::save_location)).
  ///
  /// # Errors
  ///
  /// Returns an error if the current directory cannot be determined.
  pub fn discover() -> Result<Self, EngineError> {
    let cwd = std::env::current_dir().map_err(|e| EngineError::WorkspaceError(e.to_string()))?;
    let mut workspace = Self::discover_from(&cwd)?;
    workspace.release_save_dir = super::save_location::compiled_release_save_dir();
    Ok(workspace)
  }
```

- New method and the changed ones:

```rust
  /// Where save data lives, without creating it.
  ///
  /// Release builds (`ryo2d build`) use the OS user data folder - `%APPDATA%`,
  /// `~/Library/Application Support` or `~/.local/share`, plus
  /// `[release] save_folder` or `[project] name` - or `data/` beside the game
  /// when `[release] portable = true`. Dev builds use `[paths] save_data`
  /// under the workspace root.
  pub fn save_location(&self) -> PathBuf {
    self
      .release_save_dir
      .clone()
      .unwrap_or_else(|| self.root.join(&self.config.save_data))
  }

  /// Get the absolute path to the save data directory ([`save_location`](Self::save_location)).
  ///
  /// Creates the directory if it doesn't exist.
  pub fn save_data_dir(&self) -> Result<PathBuf, EngineError> {
    let path = self.save_location();
    if !path.exists() {
      fs::create_dir_all(&path).map_err(|e| {
        EngineError::WorkspaceError(format!("Failed to create save data directory: {}", e))
      })?;
    }
    Ok(path)
  }
```

  `set_save_data_path` also sets `self.release_save_dir = None;`, and its doc gains: "This replaces the release save location too."

In `engine.rs` `Engine::new`, directly before `// Enable hot reload in debug builds ...`:

```rust
    // Tell players and developers where settings and saves are kept
    if !headless && let Ok(workspace) = super::config::Workspace::discover() {
      engine_info!(module: "Save", "Save data: {}", workspace.save_location().display());
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib -- config::save_location config::workspace 2>&1 | grep -E "test result|FAILED"`
Expected: all pass.

- [ ] **Step 5: Verify the release path end to end**

```bash
RYO_EMBED_RESOLUTION=1280x720 RYO_EMBED_SAVE_FOLDER=RyoPathCheck \
  cargo test --lib config::workspace::tests::dev_save_location_is_inside_the_workspace 2>&1 | grep -E "test result|panicked"
cargo test --lib config::workspace::tests::dev_save_location_is_inside_the_workspace 2>&1 | grep "test result"
```

Expected: the first run FAILS (with release values embedded, `discover()` points into the OS user folder, outside the workspace); the second (unset) rebuilds and PASSES. Nothing is written to disk (`save_location` doesn't create the directory).

- [ ] **Step 6: Full suite, lint, commit**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head` and `cargo clippy --workspace --all-targets 2>&1 | grep -E "^\s+--> src/(core/config/(save_location|workspace)|core/engine)\.rs"`
Expected: 0 failed; no findings in those files.

```bash
git add Cargo.toml Cargo.lock src/core/config/save_location.rs src/core/config/workspace.rs src/core/engine.rs
git commit -m "feat(config): save release builds' data in the OS user folder

- [release] portable = true keeps data/ beside the game
- The engine logs where save data lives at startup"
```

---

### Task 3: Release builds run from their bundle folder

**Files:**
- Modify: `src/core/config/save_location.rs`
- Modify: `src/core/engine.rs` (`EngineBuilder::resolve` ~3872)
- Modify: `src/cli/commands/build.rs` (`create_release_bundle`: the `data/` creation)

**Interfaces:**
- Consumes (Task 1/2): `resolve::compiled_embedded_value`.
- Produces: `pub(crate) fn bundle_dir_to_enter(is_release: bool, exe_dir: &Path, cwd: &Path) -> Option<PathBuf>`; `pub(crate) fn enter_bundle_dir()`.

- [ ] **Step 1: Write the failing tests**

In `save_location.rs`'s tests module add:

```rust
  fn folder_with(dirs: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in dirs {
      std::fs::create_dir(dir.path().join(name)).unwrap();
    }
    dir
  }

  #[test]
  fn dev_builds_stay_in_their_working_directory() {
    let bundle = folder_with(&["assets"]);
    assert_eq!(bundle_dir_to_enter(false, bundle.path(), Path::new("/home/me")), None);
  }

  #[test]
  fn release_builds_enter_their_bundle_folder() {
    for marker in ["assets", "levels", "data"] {
      let bundle = folder_with(&[marker]);
      assert_eq!(
        bundle_dir_to_enter(true, bundle.path(), Path::new("/home/me")),
        Some(bundle.path().to_path_buf()),
        "{marker}/ marks a bundle"
      );
    }
  }

  #[test]
  fn release_builds_already_in_their_folder_stay() {
    let bundle = folder_with(&["assets"]);
    assert_eq!(bundle_dir_to_enter(true, bundle.path(), bundle.path()), None);
  }

  #[test]
  fn target_release_folders_are_not_bundles() {
    let target = folder_with(&["deps", "build", ".fingerprint"]);
    assert_eq!(bundle_dir_to_enter(true, target.path(), Path::new("/projects/hopper")), None);
  }
```

- [ ] **Step 2: Run them to watch them fail**

Run: `cargo test --lib config::save_location 2>&1 | tail -10`
Expected: compile error — `bundle_dir_to_enter` doesn't exist.

- [ ] **Step 3: Implement**

In `save_location.rs`:

```rust
/// Folders `ryo2d build` puts beside the game binary.
const BUNDLE_DIRS: [&str; 3] = ["assets", "levels", "data"];

/// The folder a release build should run from: its executable's folder, when
/// that holds the bundle's `assets/`, `levels/` or `data/` and isn't already
/// the working directory. `None` for dev builds.
pub(crate) fn bundle_dir_to_enter(is_release: bool, exe_dir: &Path, cwd: &Path) -> Option<PathBuf> {
  if !is_release || exe_dir == cwd {
    return None;
  }
  BUNDLE_DIRS
    .iter()
    .any(|dir| exe_dir.join(dir).is_dir())
    .then(|| exe_dir.to_path_buf())
}

/// Run this release build from its bundle folder, so relative paths (assets,
/// levels) resolve wherever the game was launched from.
pub(crate) fn enter_bundle_dir() {
  let is_release = compiled_embedded_value("RYO_EMBED_RESOLUTION").is_some();
  let (Ok(exe), Ok(cwd)) = (std::env::current_exe(), std::env::current_dir()) else {
    return;
  };
  let Some(exe_dir) = exe.parent() else {
    return;
  };
  if let Some(dir) = bundle_dir_to_enter(is_release, exe_dir, &cwd)
    && let Err(e) = std::env::set_current_dir(&dir)
  {
    eprintln!(
      "[ryo2d] Warning: Couldn't run from the game folder {} ({e})",
      dir.display()
    );
  }
}
```

In `engine.rs`, `EngineBuilder::resolve` becomes:

```rust
  fn resolve(self, headless: bool) -> ResolvedBuild {
    // Release bundles resolve assets and levels relative to their own folder
    if !headless {
      super::config::save_location::enter_bundle_dir();
    }
    let workspace = super::config::Workspace::discover().ok();
    self.resolve_in(workspace.as_ref(), headless)
  }
```

In `src/cli/commands/build.rs` `create_release_bundle`, create `release/data/` only in portable mode:

```rust
  // Portable builds keep save data beside the game
  if workspace.config().release.portable {
    let data_dir = release_dir.join("data");
    fs::create_dir_all(&data_dir)
      .map_err(|e| CliError::Other(format!("Failed to create data directory: {}", e)))?;
  }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib config::save_location 2>&1 | grep -E "test result|FAILED"`
Expected: all pass.

- [ ] **Step 5: Full suite, lint, commit**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head` and `cargo clippy --workspace --all-targets 2>&1 | grep -E "^\s+--> src/(core/config/save_location|core/engine|cli/commands/build)\.rs"`
Expected: 0 failed; no findings in those files.

```bash
git add src/core/config/save_location.rs src/core/engine.rs src/cli/commands/build.rs
git commit -m "fix(engine): run release builds from their bundle folder

- Assets and levels load wherever a shipped game is launched from
- ryo2d build only creates release/data/ in portable mode"
```

---

### Task 4: Documentation

**Files:**
- Modify: `src/cli/commands/init.rs` (the `[release]` block of the Ryo.toml template, ~285)
- Modify: `src/cli/skills/ryo-config-save.md`, `.claude/skills/ryo-config-save/SKILL.md` (CRLF)
- Modify: `src/cli/skills/ryo-cli.md`, `.claude/skills/ryo-cli/SKILL.md` (CRLF)
- Modify: `src/core/config/AGENTS.md`, `src/cli/AGENTS.md`

**Interfaces:** none (docs only).

- [ ] **Step 1: `ryo2d init` template**

Under `[release]` (after `# logo = "assets/icon.png"`), add:

```toml
# portable = false                 # true: keep saves in data/ beside the game instead of the OS user folder
# save_folder = "MyStudio/MyGame"  # folder under the OS user data folder (default: [project] name)
```

- [ ] **Step 2: `ryo-config-save` skill (both copies, identical, CRLF)**

Add a section after the part describing `system.json` creation:

```markdown
## Where saves live

| Build | Save folder (system.json, game and world saves) |
|---|---|
| Dev (`ryo2d dev`, `cargo run`) | `[paths] save_data` under the project (default `data/`) |
| Release (`ryo2d build`) | OS user data folder: `%APPDATA%`, `~/Library/Application Support` or `~/.local/share`, plus `[release] save_folder` (default `[project] name`) |
| Release with `[release] portable = true` | `data/` beside the game |

`Workspace::save_location()` returns the folder; the engine logs it at startup. `ryo2d run` launches the release bundle like a player would, so a non-portable game saves into your real OS folder.
```

- [ ] **Step 3: `ryo-cli` skill (both copies, identical, CRLF)**

- Replace the `## Ryo.toml` sample (it shows `[window]` and `[physics]` sections that don't exist) with:

```toml
[project]
name = "my_game"
title = "My Game"
version = "0.1.0"

[system]
resolution = "1280x720"

[debug]
show_colliders = false

[release]
# logo = "assets/icon.png"
# portable = false
# save_folder = "MyStudio/MyGame"
```

- Replace the `## Configuration Priority` list with: "Startup settings resolve in one order, each layer overriding only what it sets: engine defaults → `Ryo.toml` (dev) or the values `ryo2d build` embedded from it (release) → `EngineBuilder` calls → saved player preferences in `system.json` (only with `with_user_preferences`)."
- Add a `## Release bundle` section: "`ryo2d build` writes `release/` with the game binary, `assets/`, `levels/` and `icon.png` (plus `data/` when `[release] portable = true`). The game runs from that folder wherever it's launched from, and saves to the OS user data folder unless portable — see the `ryo-config-save` skill."

- [ ] **Step 4: AGENTS.md files**

- `src/core/config/AGENTS.md`: add a `## Save Location` section: "`save_location.rs` (pure, unit-tested) decides release paths. `Workspace::discover()` applies the release save dir (OS user folder + `[release] save_folder`/`[project] name`, or `data/` beside the game when portable); `from_path`/`discover_from` stay dev-style; `set_save_data_path` replaces it. `EngineBuilder::resolve` calls `enter_bundle_dir()` so release bundles run from their own folder. Embedded keys `RYO_EMBED_PORTABLE`/`RYO_EMBED_SAVE_FOLDER` feed it."
- `src/cli/AGENTS.md`: note that `build` validates the save folder name before compiling and creates `release/data/` only in portable mode.

- [ ] **Step 5: Verify and commit**

Run: `file src/cli/skills/ryo-config-save.md .claude/skills/ryo-config-save/SKILL.md src/cli/skills/ryo-cli.md .claude/skills/ryo-cli/SKILL.md` (expect CRLF for all four), `git diff --stat` (small diffs, no whole-file rewrites), `cargo test --lib cli::commands 2>&1 | grep "test result"` (init template unchanged in behaviour).

```bash
git add src/cli/commands/init.rs src/cli/skills/ryo-config-save.md .claude/skills/ryo-config-save/SKILL.md src/cli/skills/ryo-cli.md .claude/skills/ryo-cli/SKILL.md src/core/config/AGENTS.md src/cli/AGENTS.md
git commit -m "docs: describe where release builds keep their data"
```
