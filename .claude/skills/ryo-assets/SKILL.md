---
name: ryo-assets
description: "Assets - loading images, SVG, shaders, fonts, UI files, themes and audio (clips, sound and music definitions, the mixer) through the registry (load_now and load), asset paths, handles and what keeps an asset alive, load states and loading screens, embedded assets with add, hot reload and AssetEvent, custom asset types and loaders, and the debug checker."
metadata:
  version: 1.2.0
---

# Ryo2D Assets

Everything a game reads from its `assets/` folder goes through one registry, reached as `ctx.assets()` in scenes. Every type below is in the prelude; complete signatures are in `ryo-api` (Assets). Audio files and their definitions are assets like any other; playing them is `ryo-audio`.

## 1. Loading: `load_now` and `load`

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  // At once, on this thread, dependencies too: it draws this frame
  match ctx.assets().load_now::<Image>("player.png") {
    Ok(image) => {
      ctx.world.spawn((Transform::from_xy(320.0, 180.0), Renderable::image(image)));
    }
    Err(error) => game_warn!("no player: {}", error.report()),
  }
  // On a worker thread: a handle at once, the image a few frames later
  self.backdrop = Some(ctx.assets().load::<Image>("level2/backdrop.png"));
}
```

| | `load_now::<T>(path)` | `load::<T>(path)` |
|---|---|---|
| Returns | `Result<Handle<T>, Arc<AssetError>>`, the asset already in its store | `Handle<T>` at once; the asset lands at a later frame's pump |
| Runs | on the calling thread, its dependencies too | on the worker threads (on the web, in the pump, at most 4 ms a frame) |
| Use it for | what the scene draws at once, in `on_enter`, so no frame shows a placeholder | loading screens, the next level while this one plays, anything that may appear a few frames late |
| A failure | is the `Err`, and `AssetEvent::Failed` | makes the handle's state `LoadState::Failed(error)`, and `AssetEvent::Failed` |

- `load_with` and `load_now_with` also take the loader's settings: an SVG's size (section 6), a font collection's face (`FontSettings { face: 1 }`).
- **One path is one asset.** Asking for the same path (and settings) again while the asset lives gives the same handle, loaded, loading or failed, so every scene can load what it uses and each file is read once. A failed asset is not tried again until its file changes, or nothing holds it and it is asked for again.
- `load` never fails on the spot: a bad path, or an extension no loader takes for that type, gives a failed handle (`AssetError::InvalidPath`, `NoLoader`), warned once.
- `load_now` of an asset already loading on a worker loads it here instead.
- A `load_now` that fails holds nothing: no handle, and nothing watches its path, so a file that appears later is picked up only through a `load` handle (a failed `load` loads again when its file changes). To show a file that may be missing now and present later, keep a `load` handle beside the failed `load_now`.
- **`load_now` does the whole load on the calling thread**, a large image's mip levels too, which cost more than its decode: a 2048 x 2048 PNG takes about 15 to 40 ms (its levels 13 to 28 ms of it, on an Apple M5 Pro), so a scene that waits on several large images starts late. Load large images with `load` behind a loading screen (section 4), or in an earlier scene, and keep `load_now` for what the scene draws at once.
- Systems reach the same registry: `Res<AssetServer>` (`server.load::<Image>(path)`) and the stores as `Res<Assets<Image>>`.

### Paths

Asset paths are relative to the project's `assets/` folder (an example's own `assets/`): `"player.png"` is `assets/player.png`, `"ui/menu.ui.ron"` is `assets/ui/menu.ui.ron`. Paths inside asset files (a UI file's imports and images, a theme's skin images and sounds, a bitmap font's sheet, a sound definition's clips, a music definition's intro and loop) are asset paths too. `\` becomes `/`, `.` segments go and `..` resolves within the path; an absolute path, or one that leaves `assets/`, fails with `AssetError::InvalidPath`.

### Types and files

| Type | Files | Settings |
|---|---|---|
| `Image` | `png`, `jpg`, `jpeg`, `bmp`, `gif` (its first frame), `webp`, `tga`, `tif`, `tiff`, `qoi`, `ico`, `svg` | `ImageSettings { svg: SvgSize }` |
| `Shader` | `wgsl`: a sprite shader or a post-process effect (`ryo-renderer`). The file is compiled as it loads and must define `fn sprite` or `fn effect`; WGSL for a render pass of your own wants an asset type of its own whose loader takes `wgsl` (section 8), or `include_str!` | |
| `Font` | `ttf`, `otf`, `ttc` (`ryo-text`) | `FontSettings { face: u32 }` |
| `BitmapFont` | `bitmap.ron`: `(sheet: "fonts/pixel.png", cell: (8, 8), chars: " !\"#..", family: "Pixel")`, its sheet loaded with it | |
| `UiFile` | `ui.ron` (`ryo-ui`, Files) | |
| `Theme` | `theme.ron` (`ryo-ui`, Theme) | |
| `AudioClip` | `wav`, `ogg`, `mp3`, `flac`, mono or stereo (`ryo-audio`). Up to 10 s decodes to memory as it loads; longer keeps its bytes and streams as it plays (the web decodes every clip) | `AudioClipSettings { mode: ClipMode }` (`Auto`, `Decode`, `Stream`) |
| `Sound` | `sound.ron`: how a sound plays (clips and their weights, bus, level, pitch, limits, fades, spatial settings), its clips loaded with it (`ryo-audio`) | |
| `Music` | `music.ron`: an intro and a loop, or one clip with a loop region (`ryo-audio`) | |
| `Mixer` | `mixer.ron`: the buses, effects, sends, snapshots and ducking; the engine loads `audio/mixer.mixer.ron` (`MIXER_PATH`) at start if the game has one (`ryo-audio`) | |

The loader is picked by the longest extension, so `menu.ui.ron` is a UI file, `night.theme.ron` a theme and `step.sound.ron` a sound definition. Asking for a type no loader makes from the file (`load::<Image>("menu.ui.ron")`) fails with `AssetError::NoLoader`.

## 2. Using an asset

Pass the handle to what draws or uses it: `Renderable::image(handle)`, `ParticleEmitter::new(handle)`, `ui.image(&handle)`, `SpriteShader::custom(handle)`, `ctx.fonts().set_role(FontRole::Ui, handle)`, `ctx.follow_ui_theme(handle)`, `ctx.audio().play(&sound)`, `ctx.audio().play_music(&music)`. Read the value through the registry:

```rust
let assets = ctx.assets();
if let Some(backdrop) = &self.backdrop
  && let Some(image) = assets.get(backdrop)
{
  game_info!("the backdrop is {}x{}", image.width(), image.height());
}
```

- `get(&handle) -> Option<&T>` is `None` until the asset is loaded. `get_mut(&handle) -> Option<&mut T>` changes it in place: its version moves on, so the textures, layouts and trees built from it are built again, and `AssetEvent::Modified` is sent at the next pump.
- `ctx.assets()` borrows the world: take it, use it and let it go (bind `let assets = ctx.assets();` for several calls, and drop it before using `ctx.world`).
- A `Handle<T>` is cheap to clone (a count) and compares by id. `handle.id()` is an `AssetId<T>` (`Copy`), which names the asset without keeping it alive.

## 3. What keeps an asset alive

**An asset lives while a handle to it does.** When the last handle drops, the next pump frees it (`AssetEvent::Removed` too, if it was in its store; a load that failed or never landed is freed with no event), and its texture goes with it.

- Hold what a scene uses in the scene (`backdrop: Option<Handle<Image>>`) or in what uses it: a `Renderable` holds its image, a `ParticleEmitter` its sprite, a node tree its UI file, a font role its font, a followed theme its theme. Leaving the scene drops them, and what nothing else holds is freed at the next pump; an asset the next scene loads in its `on_enter` is kept, as the scene enters before that pump runs.
- Hold what the whole game uses in a resource, which outlives every scene:

  ```rust
  #[derive(Resource)]
  struct GameArt {
    coin: Handle<Image>,
    font: Handle<Font>,
  }

  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let mut assets = ctx.assets();
    if let (Ok(coin), Ok(font)) = (
      assets.load_now::<Image>("coin.png"),
      assets.load_now::<Font>("fonts/Game.ttf"),
    ) {
      ctx.insert_resource(GameArt { coin, font });
    }
  }
  ```

- A handle you drop at once frees the asset at the next pump: loading something and throwing the handle away loads it for nothing.
- Nothing the engine draws keeps an image alive past the game's last handle: the renderer names textures by id. While the game holds an image, its texture stays however long it goes undrawn (an image off screen, a UI left alone), so drawing it again uploads nothing; only a full atlas, or more than 64 large images, frees the textures shown least recently.

## 4. Load states and loading screens

```rust
#[derive(Default)]
struct Loading {
  held: Vec<Handle<Image>>,
}

impl Scene for Loading {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let assets = ctx.assets();
    self.held = ["bg.png", "tiles.png", "hero.png"]
      .into_iter()
      .map(|path| assets.load(path))
      .collect();
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let progress = ctx.assets().progress(&self.held);
    ui.progress(progress.fraction()).show();
    if progress.is_done() {
      // The level takes the handles, so the images stay loaded
      return SceneAction::Switch(Box::new(Level { art: std::mem::take(&mut self.held) }));
    }
    SceneAction::Continue
  }
}
```

- `progress(group) -> Progress { loaded, failed, pending }` takes a group's handles (`&self.held`, `[&bg, &tiles]`) or ids; a group across types gives untyped ids, `[image.id().untyped(), font.id().untyped()]` (any `Into<UntypedAssetId>`). `fraction()` is the share settled, loaded or failed (1 for an empty group); `is_done()` is nothing pending. An id neither loaded nor loading (freed, or never asked for) is not counted.
- `state(&handle) -> LoadState`: `NotLoaded`, `Loading`, `Loaded` or `Failed(Arc<AssetError>)`, with `is_loaded()`, `is_failed()` and `error()`. `is_ready(&handle)` is loaded with every dependency loaded too (a UI file's images, a theme's skins).
- To load a group again from its files, drop its handles and load it after the next pump (the frame after): the pump frees what nothing else holds, so those files are read again, and an asset something else still holds is found loaded at once. Count the new handles; waiting for every old one to be freed waits for good if another holder keeps one.
- `stats() -> AssetStats` counts loaded, loading and failed assets per type (`types: Vec<TypeStats { name, loaded, loading, failed }>`), for a debug overlay.
- Loads land in the pump at the start of the frame (`PreUpdate`), before `ui` and `update`: a `load` asked for in `on_enter` usually lands a few frames later. When a transition entered the scene, the pump runs right after it in the same frame, so a small file can land in that frame (on the web, where the pump runs the loads itself, it often does).
- `AssetError` (`#[non_exhaustive]`): `NotFound { path }`, `Io { path, source }`, `Decode { path, source }`, `NoLoader { path, extension }`, `Dependency { path, dependency, error }`, `Cycle { path, chain }`, `InvalidPath { path, reason }`. `Display` is one line naming the path (`could not load "level.ron"`) and the causes are its `source()` chain: `error.report()` is the error with every cause on one line, as the engine's warnings print it. The engine warns once about every failure in the dev log, so a game only handles what it can do something about.

## 5. Made in code, and embedded

`add` puts an asset you made in the registry and gives its handle. It lives while the handle does, like a loaded one, and is never reloaded (it has no file):

```rust
const LOGO: &[u8] = include_bytes!("../assets/logo.png");
const GAME_FONT: &[u8] = include_bytes!("../assets/fonts/Game.ttf");
const GLOW: &str = include_str!("../assets/shaders/glow.wgsl");
const MENU: &str = include_str!("../assets/ui/menu.ui.ron");

fn embed(ctx: &mut SceneContext) -> Result<(), Box<dyn std::error::Error>> {
  let mut assets = ctx.assets();
  let dot = assets.add(Image::solid_circle(8, 255, 255, 255, 255)); // pixels made in code
  let logo = assets.add(Image::decode(LOGO)?);                      // an embedded PNG
  let font = assets.add(Font::from_bytes(GAME_FONT)?);
  let glow = assets.add(Shader::from_wgsl("shaders/glow.wgsl", GLOW)); // errors report under this name
  let menu = assets.add(UiFile::parse("ui/menu.ui.ron", MENU)?);
  // .. keep the handles (section 3)
  Ok(())
}
```

- `Image::from_rgba(pixels, width, height)`, `solid_rect`, `solid_circle` and `solid_ellipse` make images; `Image::decode(bytes)` decodes any raster format the loader reads whose bytes name it: all but TGA (it has no signature; give a TGA its file and `load` it) and SVG. `Font::from_bytes` takes `impl Into<Arc<[u8]>>`, so give it a `&[u8]` (a `const` as above, or `include_bytes!(..).as_slice()`).
- A `UiFile::parse` text's imports are asset paths, loaded when it is first built. A type of your own needs nothing registered to `add` it (section 8).
- **The web build has no files.** `load` and `load_now` fail there (`AssetError::Io` with `ErrorKind::Unsupported`, noted in the dev log at debug level rather than warned about), so embed what a web build needs. To read the file from `assets/` where there is one (debug builds reload it when saved) and fall back to the embedded copy:

  ```rust
  fn ui_file(ctx: &mut SceneContext, path: &str, embedded: &str) -> Result<Handle<UiFile>, UiFileError> {
    match ctx.assets().load_now::<UiFile>(path) {
      Err(error) if not_on_disk(&error) => Ok(ctx.assets().add(UiFile::parse(path, embedded)?)),
      loaded => Ok(loaded?),
    }
  }

  /// No file at the path, or no files at all (the web)
  fn not_on_disk(error: &AssetError) -> bool {
    match error {
      AssetError::NotFound { .. } => true,
      AssetError::Io { source, .. } => source.kind() == std::io::ErrorKind::Unsupported,
      _ => false,
    }
  }
  ```

  (A `UiFileError` comes from an `Arc<AssetError>`, so `?` turns a load error into one.)

- Or give the web build its files as its source, so every asset path loads as it would from `assets/`, imports and images included:

  ```rust
  /// The web build's assets, embedded in the binary
  #[cfg(target_arch = "wasm32")]
  fn web_assets(builder: EngineBuilder) -> EngineBuilder {
    let files = MemorySource::new();
    files.insert("ui/menu.ui.ron", include_str!("../assets/ui/menu.ui.ron"));
    files.insert("player.png", include_bytes!("../assets/player.png").as_slice());
    builder.asset_source(files)
  }
  ```

  Audio goes in the same source: the clips, the definitions, and the mixer at `MIXER_PATH` (the engine asks the source whether it has one). On the web every clip decodes in the pump, so a long one is a hitch when it lands (`ryo-audio`, The web).

## 6. SVG

An SVG is rasterised when it loads, at the size its settings ask for, and each size is an image of its own:

```rust
let fit = ImageSettings { svg: SvgSize::Fit { width: 32, height: 32 } };
let heart = ctx.assets().load_now_with::<Image, _>("icons/heart.svg", fit);
```

- `SvgSize::Native` (the default: the size the SVG gives itself), `Scale(f32)` (its own size times the factor: `Scale(2.0)` for a HiDPI icon) or `Fit { width, height }` (as large as fits the box, keeping its aspect ratio).
- One file at two sizes is two images, and saving the file reloads both. Raster files ignore the settings.

## 7. Hot reload and `AssetEvent`

In debug builds with a window the engine watches `assets/`: a saved file reloads under the same handle about 100 ms after its last write, so whatever draws it changes in place, with nothing to call. Release and headless builds do not watch; `EngineBuilder::hot_reload(true)` turns watching on in any build, and `false` turns it off.

- A reload keeps the asset's id and moves its version on: an image's texture uploads again, trees from a `.ui.ron` file are brought in line (`UiReloaded`), a followed theme applies again, a shader compiles again, text in a font lays out again.
- **A saved shader keeps drawing its last version until the new one is compiled.** A shader already drawn compiles its new version on a thread of the renderer's own (about 70 to 130 ms of GPU compile on Metal for new WGSL) and draws it from the start of the frame after, so a save brings no hitch (frames cost about 0.1 ms more while it compiles, as measured) and nothing draws plain meanwhile. A shader first drawn after the save, or one with no version that ever drew, compiles in its first frame. On the web there are no threads to compile on, so there the next frame compiles it.
- The assets that hold a reloaded one are checked again: a UI file importing the saved file, a theme whose skin image changed, a bitmap font whose sheet did, a sound definition whose clip did.
- **Audio reloads into what is playing:** a saved clip switches its loops and the music playing it where they are, a saved definition retunes its playing loops, and a saved mixer rebuilds the buses with the loops carrying on (`ryo-audio`, Hot reload).
- A reload that fails (a typo in a file) keeps the last good value and warns once, naming the file and, where its format gives one, the line and column: ``could not load "shaders/glow.wgsl": 16:3: expected `;`, found "let"``. Fix the file and save again. A shader is compiled as its file loads, so a `.wgsl` file that does not compile fails as any other file does. A removed file keeps its asset as it was.
- `ctx.assets().server().reload(path)` asks for the same reload by hand (a headless test, a console command).
- **A large image shows its new pixels after its `Modified`.** An image whose texture's levels are more than a frame's upload budget (2 MiB; a square image over about 610 px) is written into a new texture over frames, about a frame per 2 MiB (11 frames at 2048 x 2048, about 22 at 3840 x 2160), while its old pixels draw, and the new ones replace them whole. `Modified` is sent when the new value lands, at the start, so a count of reloads moves first and the picture follows a few hundred milliseconds later. A smaller image draws its new pixels in the frame it lands.

Every asset type has a message, `AssetEvent<T>`:

```rust
fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
  for event in ctx.messages::<AssetEvent<Image>>() {
    match event {
      AssetEvent::Added { id } => game_info!("{id:?} is loaded (or was added in code)"),
      AssetEvent::Modified { id } => game_info!("{id:?} changed: reloaded, a dependency reloaded, or get_mut"),
      AssetEvent::Removed { id } => game_info!("{id:?} was freed: its last handle dropped"),
      AssetEvent::Failed { id, error } => game_warn!("{id:?} will not load: {}", error.report()),
      AssetEvent::ReloadFailed { id, error } => game_warn!("{id:?} kept its last good value: {}", error.report()),
    }
  }
  SceneAction::Continue
}
```

- Compare an event's id with a handle's: `id == self.level.id()`.
- The pump sends them at the start of the frame (`PreUpdate`); `load_now` sends its `Added` or `Failed` at once. An asset's events come in the order its changes happened, `Removed` last, and only for an asset that was `Added`: a load dropped before it landed, or one that failed, is freed with no event.
- Systems read them with `MessageReader<AssetEvent<T>>`.

## 8. Your own asset types

A level format, say: a name on the first line, its background image's path on the second.

```rust
struct Level {
  name: String,
  background: Handle<Image>, // a dependency: the level holds it
}
impl Asset for Level {}

struct LevelLoader;

impl AssetLoader for LevelLoader {
  type Asset = Level;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["level"]
  }

  fn load(&self, bytes: Vec<u8>, _settings: &(), cx: &mut LoadContext<'_>) -> Result<Level, AssetError> {
    let text = String::from_utf8(bytes).map_err(|error| AssetError::decode(cx.path(), error))?;
    let mut lines = text.lines();
    let name = lines.next().unwrap_or_default().to_string();
    let background = cx.load::<Image>(lines.next().unwrap_or_default())?;
    Ok(Level { name, background })
  }
}

fn main() -> EngineResult<()> {
  EngineBuilder::new()
    .asset_loader(LevelLoader) // registers Level too
    .start_scene(Title)
    .build()
    .run()
}
```

Then `ctx.assets().load_now::<Level>("levels/one.level")`, or `load` it.

- `load` runs on a worker thread, or on the calling thread under `load_now`: it turns the bytes into the asset and loads dependencies, nothing more. A dependency loaded through `cx.load` (or `cx.load_with`) is kept by the value, as its handle in a field; the asset is inserted only once its dependencies have loaded, a dependency that fails fails it (`AssetError::Dependency`), and one that reloads makes it `Modified`.
- `check(&self, asset: &Level, cx: &CheckContext<'_>) -> Result<(), AssetError>` (optional) runs on the main thread once the dependencies are loaded, and again whenever one reloads: check the asset against them there (`cx.world().resource::<Assets<Image>>()`), and fail it with `AssetError::decode(cx.path(), error)`. An error fails the load, or keeps the last good value after a reload:

  ```rust
  fn check(&self, level: &Level, cx: &CheckContext<'_>) -> Result<(), AssetError> {
    let images = cx.world().resource::<Assets<Image>>();
    match images.get(&level.background) {
      Some(background) if background.width() < 640 => {
        Err(AssetError::decode(cx.path(), "the background is narrower than the screen"))
      }
      _ => Ok(()),
    }
  }
  ```
- `type Settings` is any `Default + Clone + Hash + Eq + Send + Sync + 'static` type (`()` for none). Settings are part of the asset's key: one file loaded with two settings (`load_with`) is two assets.
- Fail a bad file with `AssetError::decode(cx.path(), error)`, from any error type or a message, rather than panic. In native builds a loader that panics fails its asset (`AssetError::Decode`), not the game; the web build aborts on a panic, so there a panicking loader ends the game.
- `extensions` are matched without regard to case, and one written with its dot (`".level"`) is taken as `"level"`. A game loader for an extension an engine loader of the same type takes replaces the engine's.
- A type only ever made in code needs no loader: `impl Asset for Note {}`, then `ctx.assets().add(Note(..))`.

## 9. Missing images: the checker

An image that is not loaded (still loading, failed or freed) draws a magenta-and-black checker stretched over the sprite in debug builds, and nothing in release builds: you see what is missing while you develop, and players never do.

- In the UI the checker draws only where you gave a size or a rect (`ui.image(&handle).size(32.0, 32.0)`, `ui.draw_sprite_rect`): an unloaded image at its natural size is zero-sized, so a UI lays out the same in both builds.
- An image loaded with `load` shows the checker until it lands; load what a scene draws at once with `load_now`.
- The UI knows an image's size from the frame it lands; an image first loaded inside `Scene::ui` is sized from the next frame.

## 10. In tests

- `EngineBuilder::asset_source(source)` reads assets from another `AssetSource`: a `MemorySource` of the test's files (`files.insert("ui/menu.ui.ron", text)`; an `insert` over a path reloads it when hot reload is on), or a `DiskSource::new(folder)`.
- A source of your own implements `read`, and `watch` if it can tell when a file changed (it is asked once, when hot reload is on): keep the `AssetChanges` it is given and `send` each changed path from any thread, and the next pump reloads it:

  ```rust
  use std::sync::{Mutex, PoisonError};

  /// Levels a level editor sends while the game runs
  struct FromEditor {
    files: MemorySource,
    changes: Mutex<Option<AssetChanges>>,
  }

  impl FromEditor {
    fn received(&self, path: &str, bytes: Vec<u8>) {
      self.files.insert(path, bytes);
      let changes = self.changes.lock().unwrap_or_else(PoisonError::into_inner);
      if let (Some(changes), Ok(path)) = (changes.as_ref(), AssetPath::parse(path)) {
        changes.send(path); // false once the game has quit
      }
    }
  }

  impl AssetSource for FromEditor {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
      self.files.read(path)
    }

    fn watch(&self, changes: AssetChanges) -> Result<(), AssetError> {
      *self.changes.lock().unwrap_or_else(PoisonError::into_inner) = Some(changes);
      Ok(())
    }
  }
  ```
- `HeadlessEngine::assets()` and `UiHarness::assets()` are the registry as `ctx.assets()` is; loads queued with `load` land in a step's pump. `UiHarness::new(w, h).with_asset_source(source)` gives the harness its files, and `reload_ui(path)` waits until a UI file's reload lands.
- A test with no engine that needs handles takes a world with the registry, `ryo2d::testing::asset_world()`, and adds to its stores: `world.resource_mut::<Assets<Image>>().add(image)`. A store comes only from a world: `Assets` has no public constructor, since a handle from a store of its own would name another asset in the world's (the first image is the checker).
