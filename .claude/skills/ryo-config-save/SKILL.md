---
name: ryo-config-save
description: Project configuration (Ryo.toml, and ryo2d::project!() for the web), the player's settings (ctx.settings(), the game's own sections, system.json) and game saves (ctx.saves(), slots, versions and upgrades, SaveFinished), with their limits.
metadata:
  version: 2.5.0
---

# Ryo2D Configuration, Settings and Saves

You help game developers configure their projects, give players settings that last, and save and load game data. Complete signatures are in `ryo-api` (Config, settings and saves).

## Files

| File | Holds | Written by | Format |
|------|-------|------------|--------|
| `Ryo.toml` | The project: the game's defaults, debug flags, release settings, game variables | The developer | TOML |
| `system.json` | The player's settings that differ from the game's defaults, and the game's own settings sections | The engine, a second after a change | JSON |
| `<slot>.ryo-save` | One game save: a header (name, time, play time, labels, data version), then the game's data | The engine, when the game calls `ctx.saves().save` | Binary: checksummed, compressed RON |

A game never writes either file itself: settings save themselves when they change, and a save is one call. The engine does not snapshot the `World`; a game saves its own serializable structs and rebuilds entities from them on load.

## Startup Settings

Each layer overrides only what it sets, lowest first:

1. Engine defaults
2. The project: `Ryo.toml` (dev), the values `ryo2d build` embedded from it (release), or the Ryo.toml compiled in with `ryo2d::project!()` (the web, below)
3. `EngineBuilder` calls (`.resolution(..)`, `.vsync(..)`, `.display_mode(..)`, `.show_fps(..)`, ...)
4. `system.json`: the player's values, for the settings the game keeps (`.with_user_preferences(filter)`), only those the file holds

Layers 1-3 are the game's defaults. The player's file keeps only what the player changed, so a default changed later in `Ryo.toml` or on the builder reaches every setting the player never touched. Without a filter and without a game settings section, `system.json` is never read or written. Headless builds keep it in memory, never on disk.

<!-- with: struct Menu; impl Menu { fn new() -> Self { Self } } impl Scene for Menu {} -->
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

`UserPreferenceFilter::all()`, `none()`, `only(..)`, or an array, `Vec` or slice of `UserPreference` (`MasterVolume`, `MusicVolume`, `SfxVolume`, `UiVolume`, `VoiceVolume`, `AmbienceVolume`, `Resolution`, `DisplayMode`, `Vsync`, `ShowFps`, `Bindings`). Repeated calls combine. `Resolution` is the player's window size; the game's own resolution (the pixel-art target or design area) always comes from `Ryo.toml` and the builder. A setting the game does not keep still changes for the session.

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

[system]                    # the game's defaults; the player's file keeps what the player changes
resolution = "1280x720"    # the game's canvas, scaled to fit the window
pixel_art = false           # true: resolution is a pixel-art target, scaled by whole numbers
vsync = true
display_mode = "windowed"   # windowed | exclusive_fullscreen | borderless_fullscreen
master_volume = 1.0         # the player's volume settings, 0 to 1 on the audio curve (1: the mix as designed)
music_volume = 1.0
sfx_volume = 1.0
ui_volume = 1.0
voice_volume = 1.0          # dialogue
ambience_volume = 1.0
# audio_buffer_frames = 256 # the audio device's buffer: smaller is lower latency, larger safer
show_fps = false
console = false             # developer console (backtick), debug and release
target_background_fps = 0   # unfocused frame rate, 0 = target_fps
background_rendering = true # keep presenting while unfocused

[tilemap]
# tiled_project = "tiled/game.tiled-project"  # class and enum types of Tiled maps (default: project.tiled-project at the asset root)

[debug]                     # developer only, never in system.json
show_colliders = false
show_velocities = false
show_origins = false
show_frame_timing = false
disable_debug_keys = false
enable_profiler = false
logger_window = false
# dev_window_keys = true

[release]                   # read by `ryo2d build`, and by the web build for its save store
# logo = "assets/icon.png"
# portable = false
# save_folder = "MyStudio/MyGame"

[env]                       # game variables, shipped with release builds
difficulty = "normal"
```

| Section | In system.json | Read through |
|---------|----------------|--------------|
| `[project]`, `[paths]` | No | `workspace.config().name`, `.title`, `.assets`, ... |
| `[system]` | Its volumes, `display_mode`, `vsync` and `show_fps` are the defaults the player's values are kept against | `ctx.settings()` (the player's settings start from them); `show_fps` and `console` also in `workspace.config().debug` |
| `[tilemap]` | No | `workspace.config().tiled_project` |
| `[debug]` | No | `workspace.config().debug` |
| `[release]` | No | `workspace.config().release` |
| `[env]` | No | `config.env_str/env_bool/env_int/env_float(key)`, `has_env` |

- A key that a known section (`[project]`, `[paths]`, `[system]`, `[debug]`, `[release]`, `[tilemap]`) does not have warns once at startup, naming the nearest known key. A value of the wrong type warns and is ignored. `[env]` and the game's own sections are free-form.
- `audio_buffer_frames`, `resolution`, `pixel_art`, `console` and the background settings are the project's only: the player does not keep them.

## The Web: `ryo2d::project!()`

A browser has no `Ryo.toml` to read and no `ryo2d build` to embed it, so a web game compiles its own in and hands it to the builder:

<!-- with: struct Menu; impl Menu { fn new() -> Self { Self } } impl Scene for Menu {} -->
```rust
EngineBuilder::new()
  .project(ryo2d::project!()) // this crate's Ryo.toml, read as the game compiles
  .start_scene(Menu::new())
  .build()
  .run()
```

- `project!()` reads `Ryo.toml`, else `ryo.toml`, beside the crate's `Cargo.toml`; `project!("config/Game.toml")` names another path from there. Cargo rebuilds when the file changes. With no file it gives none, and the game runs on the engine's and the builder's values.
- The engine uses it only where it finds no project of its own: on the web, or natively when no Ryo.toml is in the working directory or above. Its values resolve as a `ryo2d build` of the same file would. A release build's embedded values, and a Ryo.toml on disk, come first, so the call is harmless natively.
- It also names the web's save store: `[release] save_folder`, else `[project] name`, else the window title (warned once, since every game on one site would share that store). The name is permanent once the game ships: changing it loses the players' web saves.

## Where Saves Live

| Build | Save folder (system.json and game saves) |
|-------|------------------------------------------|
| Dev (`ryo2d dev`, `cargo run`) | `[paths] save_data` under the project (default `data/`) |
| Release (`ryo2d build`) | OS user data folder (`%APPDATA%`, `~/Library/Application Support`, `~/.local/share`) plus `[release] save_folder` (default `[project] name`) |
| Release with `portable = true` | `data/` beside the game |
| Web | The browser's storage (IndexedDB), one store per game, named as above |
| Headless (tests, `RYO_HEADLESS`) | Memory, never the disk: `HeadlessEngine::saves()` reads and plants files, `EngineBuilder::memory_saves(store)` gives the store |

- `Workspace::save_location()` returns the folder; the engine logs it at startup. `ryo2d run` launches the release bundle, so a non-portable game saves into your real OS folder.
- Set `[release] save_folder` before the first release: changing it (or `[project] name` while it is unset) moves to a new, empty folder.
- A default folder name has `/` and characters Windows rejects replaced with `-`; an explicit `save_folder` is used as written (`Studio/Game` nests).
- Release builds read the embedded Ryo.toml values (including `[env]`), never a Ryo.toml on disk.

## Workspace

```rust
let workspace = Workspace::discover()?;        // finds Ryo.toml (up to 3 parents)
let config = workspace.config();
let difficulty = config.env_str("difficulty").unwrap_or("normal");
```

`Workspace` reads the project from disk (or a release build's embedded values), so it is native only: on the web `Workspace::discover()` returns an error.

## The Player's Settings

`ctx.settings()` gives the `Settings` resource (`Res<Settings>` and `ResMut<Settings>` in systems). Setting a value is all a game does: the engine applies it and saves it.

```rust
let mut settings = ctx.settings();
settings.set_volume(VolumeKind::Music, 0.5);                   // playing music follows this frame
settings.set_display_mode(DisplayMode::BorderlessFullscreen); // the window changes after the frame
let vsync = settings.vsync();
```

| Setting | Get | Set |
|---------|-----|-----|
| The six volumes | `volume(VolumeKind)` | `set_volume(VolumeKind, f32)`: clamped to 0..=1, NaN is the game's default |
| The window's size (windowed, logical pixels) | `window_size()` | `set_window_size(w, h)`: each side at least 1 |
| Display mode | `display_mode()` | `set_display_mode(DisplayMode)` |
| Vsync | `vsync()` | `set_vsync(bool)` |
| The FPS counter | `show_fps()` | `set_show_fps(bool)` |

- **A change applies the same frame**, after the fixed steps: the volumes reach the mixer at once, the window size and display mode go to the window after the frame, vsync reconfigures the surface, and the FPS counter shows or hides. A change made in a scene's `update` or `ui` applies that frame; one from an `Update` or `PostUpdate` system, the next.
- **It saves itself** a second after the last change to a setting the game keeps, and at once when the game quits or its window is hidden or minimised (on the web, when the page is hidden). A slider dragged over 60 frames writes the file once. A setting set back to the game's default leaves the file.
- **The window follows the player too:** a resize and a fullscreen toggle from the OS or F5 update `Settings`, so it always matches the window, and with `UserPreference::Resolution` kept the size the player drags the window to is saved.
- `reset(UserPreference::MusicVolume)` puts one setting back to the game's default and `reset_all()` every one (and every game section to its `Default`); bindings reset with `ctx.actions().reset_defaults()`. `is_kept(preference)` says whether the game keeps it.
- **The volume settings are points on a curve**, from 0 (silent) to 1 (the mix as designed): the gain is 40·log10(v) dB, so 0.5 is about -12 dB and 0.89 about -2 dB (`ryo-audio`, The player's volume settings).
- `DisplayMode::all()` and `display_name()`, `Resolution::common()` and `display_name()` feed settings menus.
- The UI scale is not a player setting: `ctx.request_ui_scale(..)` sets it for the session, applied after the frame.
- **A failed write** (a full disk, a read-only folder) warns once a run and sends a `SettingsSaveFailed { error }` message for each failed write. The settings still apply, and the engine tries again at the next change and when the game quits.

### Game Sections

A game's own settings (subtitles, screen shake, a difficulty) live in the same file as typed sections. Register each type under a key; it starts at its `Default`, and the file keeps only its top-level fields that differ from it:

<!-- with: use serde::{Deserialize, Serialize}; struct Menu; impl Scene for Menu {} -->
```rust
#[derive(Serialize, Deserialize)]
struct Gameplay {
  subtitles: bool,
  screen_shake: f32,
}

impl Default for Gameplay {
  fn default() -> Self {
    Gameplay { subtitles: false, screen_shake: 1.0 }
  }
}

let builder = EngineBuilder::new().game_settings::<Gameplay>("gameplay").start_scene(Menu);

// In a scene: read and change it as the engine's settings are
ctx.settings().game_mut::<Gameplay>().subtitles = true;
let shake = ctx.settings().game::<Gameplay>().screen_shake;
```

- A section type is `Default + Serialize + DeserializeOwned + Send + Sync + 'static`. Registering one makes the engine read and write `system.json` whatever the filter. `game::<T>()` of a type never registered panics, naming the fix.
- A section saves itself as the engine's settings do; `reset_all()` puts it back to its `Default`.
- A section that does not read (written by a newer build, or edited into the wrong shape) stays at its default with a warning, and the file's copy is kept until the player changes the section.
- **A section whose type changed** registers its versions as a save does (Versions, below), with `game_settings_versioned`. An older section goes to the steps as it was stored, and a field left at its default was never written, so the older type needs `#[serde(default)]` and the `Default` that release had:

<!-- with: use serde::{Deserialize, Serialize}; #[derive(Serialize, Deserialize)] struct Gameplay { subtitles: bool, screen_shake: f32 } impl Default for Gameplay { fn default() -> Self { Gameplay { subtitles: false, screen_shake: 1.0 } } } -->
```rust
/// The section as the first release kept it
#[derive(Deserialize)]
#[serde(default)]
struct GameplayV1 {
  subtitles: bool,
  shake: bool,
}

impl Default for GameplayV1 {
  // As that release had it: a field the file leaves out was at this value
  fn default() -> Self {
    GameplayV1 { subtitles: false, shake: true }
  }
}

let builder = EngineBuilder::new().game_settings_versioned::<Gameplay>(
  "gameplay",
  SaveVersions::new(2).step(1, |old: GameplayV1| Gameplay {
    subtitles: old.subtitles,
    screen_shake: if old.shake { 1.0 } else { 0.0 },
  }),
);
```

### Saved Bindings

Input actions (`ctx.actions().bind(..)`, `ryo-controller`) are rebound by the player with `ui.rebind(action)` rows in a settings screen (`ryo-ui`) or `ctx.actions().rebind(..)`. To keep them between runs, keep `UserPreference::Bindings`:

<!-- with: struct Menu; impl Menu { fn new() -> Self { Self } } impl Scene for Menu {} -->
```rust
EngineBuilder::new()
  .with_user_preferences([UserPreference::MasterVolume, UserPreference::Bindings])
  .start_scene(Menu::new())
  .build()
  .run()
```

- At startup the engine loads the saved bindings: actions already bound switch to them at once, the rest when the game binds them (so `bind` in a scene's `on_enter` keeps the player's choice).
- When the bindings change, the engine writes those that differ from the defaults with the settings, a second later, as action name to binding texts: `"bindings": { "jump": ["Key:KeyK", "Pad:South"] }`. Saved bindings of actions the game has not bound yet (a level's, while the player rebinds from the title screen) are written back as they were loaded, and an action the player rebinds back to its defaults leaves the file. `ctx.actions().reset_defaults()` empties the key, and the key is left out when nothing differs.
- A rebind row captures only bindings that survive a save and load (`Binding::is_saveable()`); a saved text that no longer parses is skipped with a warning, and an action none of whose saved texts parse keeps its defaults.
- Without `Bindings` in the filter, bindings last for the session.

### `system.json`

```json
{
  "format_version": 3,
  "music_volume": 0.5,
  "display_mode": "borderless_fullscreen",
  "bindings": {
    "jump": ["Key:KeyK", "Pad:South"]
  },
  "game": {
    "gameplay": { "version": 1, "value": { "subtitles": true } }
  }
}
```

- The keys: `format_version` (3), the six volumes (`master_volume` .. `ambience_volume`, to two decimals), `resolution` (the window size, `"1280x720"`), `display_mode`, `vsync`, `show_fps`, `bindings` and `game`. Only the kept settings that differ from the game's defaults are there; a first run writes no file until the player changes something.
- Keys the engine does not know (a newer build's, or added by hand) are kept as they are when it writes.
- A bad value (the wrong type, a volume outside 0 to 1) falls back to the default with a warning, and the rest of the file applies.
- A damaged file is set aside as `system.json.damaged` and the backup is used if it is whole, else the defaults, with a warning. A file from a newer engine is read for the keys this one knows and never overwritten in that run.
- **Files from older engines** (format 1, before `format_version`, and format 2) convert as they load and are written as format 3 at the first save. Format 1's volumes were amplitudes, heard at 20·log10(v) dB: each becomes √v, heard at the same dB on today's curve, so a player's settings keep their loudness. Keys equal to the defaults drop out at that first save.
- Older engines copied `[env]` into the file. The values a player's file holds that differ from the project's `[env]` become the section `env`; a game that used them registers a section named `env` whose `Default` mirrors its Ryo.toml `[env]`, since the converted section holds only the keys that differed, merged over `T::default()`:

<!-- with: use serde::{Deserialize, Serialize}; -->
```rust
/// Ryo.toml has `[env] difficulty = "normal"`
#[derive(Serialize, Deserialize)]
struct Env {
  difficulty: String,
}

impl Default for Env {
  fn default() -> Self {
    Env { difficulty: "normal".to_string() }
  }
}

let builder = EngineBuilder::new().game_settings::<Env>("env");
```

## Game Saves

<!-- with: use serde::{Deserialize, Serialize}; -->
```rust
#[derive(Serialize, Deserialize)]
struct Progress {
  level: u32,
  score: u64,
  unlocked: Vec<String>,
}

let progress = Progress { level: 3, score: 1200, unlocked: vec!["dash".to_string()] };
ctx.saves().save("slot1", progress)?;                      // queued: written on the save thread
let progress: Progress = ctx.saves().load("slot1")?.data;  // Loaded<Progress>, at once
let slots = ctx.saves().list()?;                           // each slot with its header
ctx.saves().delete("slot1")?;                              // the save and its backup
```

- **A slot name** is 1 to 64 characters of `a`-`z`, `0`-`9`, `-` and `_` (not a Windows device name such as `con` or `com1`), so one name is one slot on every disk and in the browser; anything else is `SaveError::InvalidSlot`. A display name goes in the header (`save_with`, below). A save an older engine wrote under another name still loads, lists and deletes on native, but cannot be written again under that name.
- **`save` takes the data by value** (`T: Serialize + Send + 'static`): build a snapshot struct and hand it over. The call only checks the slot and queues the save, and fails at once only for a bad slot name or no storage (`Unavailable`). The data is serialized, compressed, checksummed and written on the save thread, and saves of one slot are written in call order.
- **`SaveFinished { slot, op, result }`** reports each save and delete once, in a frame after its call: `result` is `Ok`, or the `SaveError` (shared as an `Arc`). Saves of one slot that follow each other faster than they are written write only the last; the ones before it report `Ok`. `is_writing(slot)` is true until the report.
- **`load::<T>(slot)` is immediate** and returns `Loaded { data, header, from_backup, upgraded_from }`. It sees what is queued: a load right after a save returns the data just saved. Load on a loading screen, since it reads and decodes the whole save.
- **`list()` reads only the headers**: every slot with its `SaveHeader`, sorted by slot name, queued saves included; a slot with no header to show (damaged, or from a newer build) is left out. `info(slot)` is one header, `exists(slot)` whether a save is kept or queued.
- **`delete(slot)`** removes the save and its backup, after any save of the slot queued before it, and reports a `SaveFinished` with `SaveOp::Delete`.
- **Errors** (`SaveError`, each keeping its cause as its `source()`): `Io { path, source }` (the file could not be read or written), `Corrupt { slot, reason }` (the save and its backup are both damaged), `NewerVersion { found, current }`, `NotFound(slot)`, `InvalidSlot(slot)`, `StorageFull`, `Unavailable` (nowhere to save), `Serialize(source)` (the data's `Serialize` failed: the previous save stays), `Deserialize { slot, source }` (the save does not read as the type asked for, or an upgrade step failed).
- **From a system**, take `saves: Saves` as a parameter: the same methods. Systems that save run beside each other.
- Serde is not re-exported: add `serde = { version = "1", features = ["derive"] }` to the game.

A slot list with names and play times:

<!-- with: #[derive(serde::Serialize)] struct Progress { level: u32 } -->
<!-- given: ui: &mut UiContext, progress: Progress, played_secs: u64 -->
```rust
// Saving with what a slot list shows
let info = SaveInfo::new().name("The Sunken Forest").play_time(played_secs).label("chapter", "2");
ctx.saves().save_with("slot1", progress, info)?;

// The list, one button a slot
for slot in ctx.saves().list().unwrap_or_default() {
  let name = slot.header.name.as_deref().unwrap_or(&slot.slot);
  let minutes = slot.header.play_time / 60;
  if ui.button(format!("{name} ({minutes} min)")).id(&slot.slot).fill_width().clicked() {
    // load slot.slot
  }
}
```

### Crash Safety and Backups

- **A save never destroys the last good one.** It writes a temporary file, flushes it to disk, makes the previous save the backup (only if it is whole) and renames the new file over the slot. A crash, power cut or full disk at any point leaves a whole copy of the old or the new save, and a load finds it.
- **A damaged save loads its backup**, the save before it, with `from_backup` set and a warning; when both are damaged the load is `Corrupt`.
- **A save from a newer build** (a newer data version or file format) loads as `NewerVersion { found, current }`, and a save over a newer format's file is refused, leaving it alone.
- **On quit** the engine finishes the queued saves, warning after 3 s and giving up after 10 s.
- Saves from older engines load as version 1 and are written in today's format at their next save.

### Versions and Upgrades

Each saved type has a version. A type never registered is version 1, as is every save from before the engine recorded versions. When a type changes shape, register its versions: the current one and a typed step from each older version, which the loader runs in order.

<!-- with: use serde::{Deserialize, Serialize}; -->
```rust
#[derive(Serialize, Deserialize)]
struct HeroV1 {
  hp: u32,
}

#[derive(Serialize, Deserialize)]
struct HeroV2 {
  max_hp: u32,
  class: String,
}

/// Today's hero, version 3
#[derive(Serialize, Deserialize)]
struct Hero {
  max_hp: u32,
  class: String,
  strength: u32,
  #[serde(default)] // added without a new version: older saves load with 0
  gold: u32,
}

fn hero_versions() -> SaveVersions<Hero> {
  SaveVersions::new(3)
    .step(1, |old: HeroV1| HeroV2 { max_hp: old.hp, class: "Wanderer".to_string() })
    .step(2, |old: HeroV2| Hero { max_hp: old.max_hp, class: old.class, strength: 5, gold: 0 })
}

let builder = EngineBuilder::new().save_versions(hero_versions());
```

- Step 1 turns a version-1 value into version 2, step 2 a version-2 value into version 3, and the last step yields the current type. `load::<Hero>` runs the steps from the save's version on, and `upgraded_from` says where it started; the next save writes version 3.
- **A gap panics at the call**, naming the type: a missing step, two steps from one version, a step out of range, or version 0. So a broken chain fails as the game starts, never at a player's load.
- Data passes between steps serialized, so a step's result and the next step's input need only share a shape.
- **Small additions need no new version:** `#[serde(default)]` on a new field lets older saves load. An older type kept for a step must read every save of its version, so a field added to it during that version needs `#[serde(default)]` too.
- **Test each step against real old data.** Registration checks the chain's shape, but whether a step reads what an older build wrote is found only at a load. Keep a save from each shipped build beside the tests and load it:

<!-- with: #[derive(serde::Serialize, serde::Deserialize)] struct Hero { max_hp: u32 } fn hero_versions() -> SaveVersions<Hero> { SaveVersions::new(1) } struct Title; impl Scene for Title {} -->
```rust
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, MemorySaves};

#[test]
fn a_save_from_the_first_release_still_loads() {
  // Copied from the first release's save folder (data/ in a dev build)
  let store = MemorySaves::new();
  store.put("hero.ryo-save", std::fs::read("tests/saves/hero-v1.ryo-save").unwrap());
  let builder = EngineBuilder::new().memory_saves(store).save_versions(hero_versions()).start_scene(Title);
  let Some(engine) = HeadlessEngine::new_or_skip(builder) else {
    return; // no GPU
  };
  let hero = SavesView::new(engine.world()).load::<Hero>("hero").unwrap();
  assert_eq!(hero.upgraded_from, Some(1));
}
```

## Patterns

### Settings Screen

The widgets edit copies of the settings, and a setting is set only when its widget changed, so a frame with no change costs the engine nothing:

```rust
struct SettingsScreen;

impl Scene for SettingsScreen {
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let mut settings = ctx.settings();
    let mut volume = settings.volume(VolumeKind::Master);
    let mut moved = false;
    let mut picked = None;
    ui.panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(300.0)
      .gap(8.0)
      .show(|ui| {
        moved = ui
          .slider(&mut volume, 0.0..=1.0)
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
    if moved {
      settings.set_volume(VolumeKind::Master, volume);
    }
    if let Some(mode) = picked {
      settings.set_display_mode(mode);
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

<!-- with: #[derive(Clone, serde::Serialize)] struct Progress { level: u32 } -->
<!-- self: autosave_timer: f32, progress: Progress -->
```rust
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
  self.autosave_timer += dt;
  if self.autosave_timer >= 60.0 {
    self.autosave_timer = 0.0;
    ctx.saves().save("autosave", self.progress.clone()).ok();
  }
  SceneAction::Continue
}
```

From a system, with the `Saves` parameter:

<!-- with: use serde::Serialize; -->
```rust
#[derive(Resource, Serialize, Clone)]
struct Score(u32);

fn save_score(saves: Saves, score: Res<Score>) {
  if score.is_changed() {
    // Saves of one slot faster than the disk only write the last
    let _ = saves.save("score", score.clone());
  }
}
```

### Telling the Player

<!-- self: saved_toast: f32 -->
```rust
// Each save and delete reports once, in a later frame
for done in ctx.messages::<SaveFinished>() {
  if let Err(error) = &done.result {
    game_warn!("saving {} failed: {error}", done.slot);
  } else if done.op == SaveOp::Save {
    self.saved_toast = 2.0; // "Saved" on screen for two seconds
  }
}
```

### Rebuilding the World From a Save

Save what you need to recreate entities (positions, health, ids) in your own struct and spawn from it in `on_enter`:

<!-- with: use serde::{Deserialize, Serialize}; -->
```rust
#[derive(Serialize, Deserialize)]
struct LevelSave {
  enemies: Vec<(f32, f32, i32)>,
}
```

<!-- with: #[derive(serde::Deserialize)] struct LevelSave { enemies: Vec<(f32, f32, i32)> } #[derive(Component)] struct Enemy { health: i32 } -->
```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  if let Ok(save) = ctx.saves().load::<LevelSave>("level") {
    for (x, y, health) in save.data.enemies {
      ctx.world.spawn((Transform::from_xy(x, y), Enemy { health }));
    }
  }
}
```

## Limits

- **On the web a save costs its frame:** there are no threads, so the frame that calls `save` serializes, compresses and writes it, and saves of one slot are not coalesced. Keep web saves small, or save at quiet moments.
- **Closing a browser tab right after a save may lose it:** browsers do not wait for storage writes at unload. `SaveFinished` arrives once the write has landed.
- **The web's first scene waits for the browser's store:** up to 2 s for it to open, then for every save in it to be read, however long that takes. Storage that is blocked or slow to open (some private modes) starts the game with saves `Unavailable` and settings lasting only the session, with one warning; a store holding a lot delays the first scene while it is read.
- **A saved fullscreen is not requested when a web game loads:** browsers enter fullscreen only from the player's gesture. The page sizes the canvas, so the window size is not kept there, and vsync does nothing on the web.
- **Storage is per site:** games served from one origin (itch.io, `trunk serve` on localhost) share the browser's storage, so give each its own `[release] save_folder` or `[project] name`.
- **The native guarantees assume a local disk:** a network folder can make renames and disk syncs slow or not atomic.
- **Two copies of a game saving one slot at once** can fail each other's saves (`SaveFinished` reports an `Io` error); the folder still ends with a whole save.
- A save or settings write that fails as the game quits is not reported: no frame is left to send the message.

## Best Practices

1. Keep `system.json` player-editable; never commit it (or `data/`).
2. Debug flags belong in `[debug]`; `[env]` ships with release builds, so set dev-only switches to `false` before release.
3. Save structured data you control rather than component dumps; `Transform` and `Velocity` derive serde if you do want them.
4. Choose `with_user_preferences` keys deliberately: a kept setting the player changed overrides the builder and `Ryo.toml` from then on, while the others keep following them.
5. Give a saved type `#[serde(default)]` on fields you add, a new version when its shape changes, and a test that loads a save from each release.
