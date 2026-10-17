---
name: ryo-config-save
description: Project configuration (Ryo.toml), player settings (system.json, SystemState) and game saves (SaveManager, GameSaveData).
metadata:
  version: 2.1.0
---

# Ryo2D Configuration and Saves

You help game developers configure their projects and save and load data. Complete signatures are in `ryo-api`.

## Files

| File | Purpose | Who edits | Format |
|------|---------|-----------|--------|
| `Ryo.toml` | Developer defaults, debug flags, release settings, game variables | Game developer | TOML |
| `system.json` | Player settings (`SystemState`) | Player (or the game's settings menu) | JSON |
| `<slot>.ryo-save` | Game data (`GameSaveData<T>`) | The game | Compressed binary |

There are two kinds of save: system state (`system.json`) and game data (`*.ryo-save`). The engine does not snapshot the `World`; a game saves its own serializable structs and rebuilds entities from them on load.

## Startup Settings

Each layer overrides only what it sets, lowest first:

1. Engine defaults
2. `Ryo.toml` (dev) or the values `ryo2d build` embedded from it (release)
3. `EngineBuilder` calls (`.resolution(..)`, `.vsync(..)`, `.display_mode(..)`, `.show_fps(..)`, ...)
4. `system.json`, only with `.with_user_preferences(filter)`, only keys present in the file and in the filter

Without `with_user_preferences`, `system.json` is never read or written. With it, a missing file is seeded from layers 1-3 holding only the filtered keys (plus `env` when `[env]` exists); after that the file wins over later builder or `[system]` changes, so delete it in dev to re-seed. Headless builds never touch it.

```rust
EngineBuilder::new()
  .title("My Game")
  .resolution(1280, 720)
  .fixed_hz(60)
  .with_user_preferences([UserPreference::MasterVolume, UserPreference::MusicVolume])
  .start_scene(Menu::new())
  .build()
  .run()
```

`UserPreferenceFilter::all()`, `none()`, `only(..)`, or an array/`Vec`/slice of `UserPreference` (`MasterVolume`, `MusicVolume`, `SfxVolume`, `Resolution`, `DisplayMode`, `Vsync`, `ShowFps`, `Bindings`). A saved `Resolution` sizes the window; the game's own resolution (the pixel-art target or design area) comes from `Ryo.toml` and the builder. Repeated calls combine.

The fixed update rate is builder-only: `EngineBuilder::fixed_hz(hz)` (default 60). There is no Ryo.toml key for it.

## Ryo.toml

```toml
[project]
name = "My Game"
title = "My Awesome Game"   # window title (defaults to name)
version = "0.0.1"

[paths]
entry_point = "main.rs"
save_data = "data"          # dev save folder
assets = "assets"

[system]                    # defaults for player settings
resolution = "1280x720"    # the game's canvas, scaled to fit the window
pixel_art = false           # true: resolution is a pixel-art target, scaled by whole numbers
vsync = true
display_mode = "windowed"   # windowed | exclusive_fullscreen | borderless_fullscreen
master_volume = 0.8
music_volume = 1.0
sfx_volume = 1.0
show_fps = false
console = false             # developer console (backtick), debug and release
target_background_fps = 0   # unfocused frame rate, 0 = target_fps
background_rendering = true # keep presenting while unfocused

[debug]                     # developer only, never in system.json
show_colliders = false
show_velocities = false
show_origins = false
show_frame_timing = false
disable_debug_keys = false
enable_profiler = false
logger_window = false
# dev_window_keys = true

[release]                   # read by `ryo2d build`
# logo = "assets/icon.png"
# portable = false
# save_folder = "MyStudio/MyGame"

[env]                       # game variables, shipped with release builds
difficulty = "normal"
```

| Section | In system.json | Read through |
|---------|----------------|--------------|
| `[project]`, `[paths]` | No | `workspace.config().name`, `.title`, `.assets`, ... |
| `[system]` | Yes (seeds it) | `workspace.config().system` (`SystemDefaults`) |
| `[debug]` | No | `workspace.config().debug` |
| `[release]` | No | `workspace.config().release` |
| `[env]` | Seeds its `env` | `config.env_str/env_bool/env_int/env_float(key)`, `has_env` |

## Where Saves Live

| Build | Save folder (system.json and game saves) |
|-------|------------------------------------------|
| Dev (`ryo2d dev`, `cargo run`) | `[paths] save_data` under the project (default `data/`) |
| Release (`ryo2d build`) | OS user data folder (`%APPDATA%`, `~/Library/Application Support`, `~/.local/share`) plus `[release] save_folder` (default `[project] name`) |
| Release with `portable = true` | `data/` beside the game |

- `Workspace::save_location()` returns the folder; the engine logs it at startup. `ryo2d run` launches the release bundle, so a non-portable game saves into your real OS folder.
- Set `[release] save_folder` before the first release: changing it (or `[project] name` while it is unset) moves to a new, empty folder.
- A default folder name has `/` and characters Windows rejects replaced with `-`; an explicit `save_folder` is used as written (`Studio/Game` nests).
- Release builds read the embedded Ryo.toml values (including `[env]`), never a Ryo.toml on disk.

## Workspace

```rust
let workspace = Workspace::discover()?;        // finds Ryo.toml (up to 3 parents)
let config = workspace.config();
let difficulty = config.env_str("difficulty").unwrap_or("normal");
let saves = workspace.create_save_manager()?;  // SaveManager over the save folder
```

## System State

`SystemState` holds `master_volume`, `music_volume`, `sfx_volume`, `width`, `height`, `display_mode: DisplayMode`, `vsync: Option<bool>`, `show_fps: Option<bool>`, `bindings` (the player's input bindings, below) and `env` (typed JSON values). In the file the resolution is one `"1280x720"` key.

```rust
let mut settings = saves.load_or_create_system(&workspace.config().system)?;
settings.master_volume = 0.5;
settings.display_mode = DisplayMode::BorderlessFullscreen;
settings.set_env("language", "en");
saves.save_system(&settings)?;
```

- `load_or_create_system(&defaults)` creates the file from `[system]` when missing and merges new default keys into an existing one; `load_or_create_system_filtered(&defaults, &filter)` writes only the filtered keys to a new file.
- `save_system` updates keys already in the file and adds a missing key only when its value differs, so a narrow file keeps its shape.
- `DisplayMode::all()` and `display_name()`, `Resolution::common()` and `display_name()` feed settings menus.
- Applying a change at runtime is separate from saving it: `ctx.request_display_mode(mode)`, `ctx.request_resolution(w, h)` (resizes the window), `ctx.request_display_change(w, h, mode)`, `ctx.request_ui_scale(..)` (applied after the frame), and `ctx.audio.set_master_volume(v)` / `set_music_volume` / `set_sfx_volume`.

## Saved Bindings

Input actions (`ctx.actions().bind(..)`, `ryo-controller`) are rebound by the player with `ui.rebind(action)` rows in a settings screen (`ryo-ui`) or `ctx.actions().rebind(..)`. To keep them between runs, enable `UserPreference::Bindings`:

```rust
EngineBuilder::new()
  .with_user_preferences([UserPreference::MasterVolume, UserPreference::Bindings])
  .start_scene(Menu::new())
  .build()
  .run()
```

- At startup the engine loads the saved bindings: actions already bound switch to them at once, the rest when the game binds them (so `bind` in a scene's `on_enter` keeps the player's choice).
- Whenever the bindings change, the engine writes those that differ from the defaults to `system.json`, as action name to binding texts: `"bindings": { "jump": ["Key:KeyK", "Pad:South"] }`. Saved bindings of actions the game has not bound yet (a level's, while the player rebinds from the title screen) are written back as they were loaded, and an action the player rebinds back to its defaults leaves the file. `ctx.actions().reset_defaults()` empties the key, and the key is left out when nothing differs.
- A rebind row captures only bindings that survive a save and load (`Binding::is_saveable()`); a saved text that no longer parses is skipped with a warning, and an action none of whose saved texts parse keeps its defaults.
- `SystemState::bindings` is a `BTreeMap<String, Vec<String>>` if a game wants to read or edit the file itself. Without `Bindings` in the filter, bindings last for the session. Headless runs never read or write them.

## Game Data

```rust
#[derive(Serialize, Deserialize)]
struct Progress {
  level: u32,
  score: u64,
  unlocked: Vec<String>,
}

saves.save_game("slot1", &progress)?;                        // slot1.ryo-save
let progress: Progress = saves.load_game("slot1")?.data;    // GameSaveData<Progress>
let slots = saves.list_game_saves()?;
saves.delete_game("slot1")?;
```

- `GameSaveData<T>` wraps the data with `timestamp` (Unix seconds), `name`, `play_time` (seconds) and `metadata`; write them with `save_game_with_info(slot, GameSaveData::new(&data).with_name("Forest").with_play_time(secs).with_metadata("chapter", "2"))`.
- A slot name with a path separator, `..` or a leading dot is `SaveError::InvalidSlot`; a missing slot is `SaveError::NotFound`.
- `set_compression_level(0..=9)` (default 6). `save_raw(filename, &data)` / `load_raw(filename)` write a custom compressed file; `compress_data` / `decompress_data` work on bytes.
- Serde is not re-exported: add `serde = { version = "1", features = ["derive"] }` to the game.

## Patterns

### Settings Screen

```rust
struct Settings {
  saves: SaveManager,
  state: SystemState,
}

impl Scene for Settings {
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let mut volume_changed = false;
    let mut picked = None;
    ui.panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(300.0)
      .gap(8.0)
      .show(|ui| {
        volume_changed = ui
          .slider(&mut self.state.master_volume, 0.0..=1.0)
          .step(0.05)
          .label("Volume")
          .show_value()
          .show()
          .changed();
        for (i, mode) in DisplayMode::all().iter().enumerate() {
          if ui.button(mode.display_name()).id(i).fill_width().clicked() {
            picked = Some(*mode);
          }
        }
      });
    if volume_changed {
      ctx.audio.set_master_volume(self.state.master_volume);
      self.saves.save_system(&self.state).ok();
    }
    if let Some(mode) = picked {
      self.state.display_mode = mode;
      ctx.request_display_mode(mode);
      self.saves.save_system(&self.state).ok();
    }
    if ui.back() {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }
}
```

A tabbed settings screen with rebinding rows is `ryo-ui`'s settings recipe.

### Autosave

`update` runs every fixed step with the constant timestep, so a timer in it counts game time (it stops while paused).

```rust
fn update(&mut self, _ctx: &mut SceneContext, dt: f32) -> SceneAction {
  self.autosave_timer += dt;
  if self.autosave_timer >= 60.0 {
    self.autosave_timer = 0.0;
    self.saves.save_game("autosave", &self.progress).ok();
  }
  SceneAction::Continue
}
```

### Rebuilding the World From a Save

Save what you need to recreate entities (positions, health, ids) in your own struct and spawn from it in `on_enter`:

```rust
#[derive(Serialize, Deserialize)]
struct LevelSave {
  enemies: Vec<(f32, f32, i32)>,
}

fn on_enter(&mut self, ctx: &mut SceneContext) {
  if let Ok(save) = self.saves.load_game::<LevelSave>("level") {
    for (x, y, health) in save.data.enemies {
      ctx.world.spawn((Transform::from_xy(x, y), Enemy { health }));
    }
  }
}
```

## Best Practices

1. Keep `system.json` player-editable; never commit it.
2. Debug flags belong in `[debug]`; `[env]` ships with release builds, so set dev-only switches to `false` before release.
3. Save structured data you control rather than component dumps; `Transform` and `Velocity` derive serde if you do want them.
4. Choose `with_user_preferences` keys deliberately: a key in the filter lets the file override the builder from then on.
