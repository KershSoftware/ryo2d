# Asset Registry (5a) Design

## Overview

Sub-project 5a of the professional-grade overhaul replaces the engine's per-type asset managers with one typed asset registry, at parity with Bevy's and Godot's: typed handles with generations, loaders registered by extension, load states with real errors, automatic lifetimes, dependency-aware readiness, and debounced hot reload under the same handle. Every game and demo moves onto it. Audio is the next sub-project (5b) and becomes a registry asset there; tilemaps join when they are rewritten as a feature module.

**Why.** Today (main `a0bf1800`, survey of 2026-10-23):

- `AssetManager` caches sprites, shaders and UI files in hash maps keyed by canonical path, with a `canonicalize` syscall on every lookup. There is no generic handle, no loader registry and no dependency tracking.
- Fonts (`Fonts::load`/`add_bytes`), themes (`Theme::load`, polled once a second), SVG (`Sprite::load_svg*`, which bypass the manager) and audio each load their own way. Games mostly call `Sprite::load` directly and bypass the cache.
- `SpriteHandle` is `Arc<RwLock<Sprite>>` with `.expect("poisoned")` on every accessor, and a process-wide serial, so two handles made from one file are two textures.
- Errors are `String`s (`LoadingState::Failed(String)`, `AssetLoadFailed { error: String }`, `SpriteError::DecodeError { reason: String }`).
- Unloading is manual (`gc_sprites`, never called), and the renderer's texture registry holds a handle clone for 600 frames after the last draw, so nothing can be freed sooner.
- Hot reload decodes sprites synchronously on the main thread with no debounce. It covers sprites, shaders and UI files; fonts and themes use other mechanisms or none.
- Only sprites can load asynchronously, and a synchronous load made while an asynchronous one is in flight is overwritten by a second handle.
- On the web, no game can load a file.

**Owner decisions (2026-10-23).**
- Purpose: one professional registry, built so the web and release packaging can plug in next.
- Scope: images (raster and SVG), shaders, fonts (outline and bitmap), UI files and UI themes. Every game, demo, template and skill moves. Not tilemaps (their loader is rewritten later) and not audio (5b).
- Success: everything is on the registry; hot reload causes no hitch (decoded off the main thread, measured); lifetimes are proven (freed when unheld, GPU memory included; stale handles safe); and a source seam, proven by an in-memory source in the tests.
- Approach A: the engine's own registry, shaped like Bevy's, on its standalone bevy_ecs and its own frame loop. Rejected: adopting `bevy_asset` (it brings Bevy's App, plugins, `bevy_tasks` and reflection, and its loader API changes most releases) and wrapping `assets_manager` (its cache keeps assets forever and its handles borrow the cache).
- The pixel buffer is renamed `Sprite` → `Image`, so "sprite" names only what is drawn.
- An image that is not loaded, or failed, draws a magenta checker in debug builds and nothing in release builds.

## Handles, ids and stores

- **`AssetId<T>`** is a slot index and a slot generation (two `u32`), `Copy`, typed with `PhantomData<fn() -> T>`. A slot's generation moves on when the slot is reused, so a stale id finds nothing, never a newer asset. A weak reference is an `AssetId<T>`.
- **`Handle<T>`** is the strong reference: `Arc` of an inner holding the id and a drop sender. Cloning bumps the count; dropping the last clone sends the id on a channel. It takes no lock to read its id. `Handle<T>` hashes and compares by id.
- **`Assets<T>`** is one bevy resource per asset type, a slot table:
  - `get(id) -> Option<&T>`, `get_mut(id) -> Option<&mut T>` (bumps the slot's content version);
  - `add(value) -> Handle<T>` for assets made in code;
  - `version(id) -> Option<u64>`: the content version, which the renderer, the text cache and UI trees compare to decide what to rebuild. It replaces `SpriteHandle`'s and `UiFile`'s own generation counters.
- **Only handles keep assets alive.** The path cache, the renderer, the text system and dependents' bookkeeping hold ids. A component, resource or scene struct that holds a handle keeps its asset.
- **`Asset`** is a marker trait (`Send + Sync + 'static`). The asset types in 5a are `Image`, `Shader`, `Font`, `BitmapFont`, `UiFile` and `Theme`.

## Loading

- **`AssetServer`** (a resource) maps asset paths to ids per type, holds the loaders and the sources, and feeds the workers.
- **Paths** are relative to the asset root: `Workspace::assets_dir()` from `Ryo.toml`, `assets/` by default. A game writes `load("turret.png")`. Paths are normalised lexically once (separators, `.` and `..`), with no syscall per lookup.
- **Sources.** `AssetSource` reads the bytes of a path and may watch for changes. 5a ships `DiskSource` (the asset root) and `MemorySource` (a map of paths to bytes, used by the tests). Embedded and web sources come next.
- **Loaders.** `AssetLoader` declares:
  - its extensions (multi-part ones such as `ui.ron` included; the longest registered match wins);
  - its asset type;
  - a `Settings` type (`Default + Clone + Hash + Eq + Send + Sync`);
  - `load(bytes, &settings, &mut LoadContext) -> Result<Asset, AssetError>`. The `LoadContext` loads dependencies and records them.

  The engine registers its own loaders, and a game adds its own with `EngineBuilder::asset_loader(..)`. A path with its settings is the cache key, so one file loaded at two settings is two assets.
- **Workers.** A pool of at most four threads (fewer on small machines) reads and decodes, over crossbeam channels. On wasm there are no threads: loads run in the pump within a per-frame time budget, so the web build compiles and works with the sources it has.
- **The pump** is the engine's existing pre-update asset step (`asset_pump_system`). Each frame it:
  - inserts finished assets and sets their states;
  - applies reloads;
  - frees assets whose last handle dropped;
  - sends `AssetEvent<T>` messages (`Added`, `Modified`, `Removed`, `Failed`, `ReloadFailed`).

  With nothing to do it costs a few channel polls.
- **`load_now`** decodes on the calling thread and inserts at once, for code that needs the value immediately (as today's synchronous loads). If a worker is loading the same key, its result is discarded when it arrives.
- **Load states.**
  - Each asset is `NotLoaded`, `Loading`, `Loaded` or `Failed(Arc<AssetError>)`.
  - Recursive readiness (`is_ready`) also counts dependencies, and reports the first that failed.
  - `progress(&handles)` counts loaded, failed and pending for a group. It replaces `start_loading` and `loading_progress`.
- **Errors.** `AssetError` has these variants, each with the asset path and a `source()` chain:
  - `NotFound`;
  - `Io { source: io::Error }`;
  - `Decode { source: Box<dyn Error + Send + Sync> }`;
  - `NoLoader { extension }`;
  - `Dependency { dependency, error: Arc<AssetError> }`;
  - `Cycle` (UI file imports).
- **Not loaded yet.** `get` returns `None` until an asset is loaded. A `Renderable` whose image is not loaded, or failed, draws a magenta checker in debug builds and nothing in release. A failure is logged once and its event sent.

## Lifetimes and the GPU

- **Freeing.**
  - Each slot keeps a `Weak` to its handle's inner. When a drop arrives, the pump frees the asset only if the `Weak` no longer upgrades, so a `load` that picked the asset up in between keeps it.
  - Freeing removes the value and its path entry, sends `Removed`, and drops the asset's handles on its dependencies.
- **Images.**
  - The texture registry is keyed by `AssetId<Image>` and holds no handle. On `Removed`, the image's atlas region or own texture is released at once; the 600-frame TTL no longer applies to images.
  - Re-uploads follow the content version.
  - Render targets and named pixel textures keep their current paths.
- **Fonts.** A freed font's faces leave the text system, its glyphs leave the glyph cache, and a glyph page left empty is freed.
- **Shaders.** A freed shader's compiled pipelines are dropped at once.
- **UI files.** Every node tree built from a file holds its handle (`TreeSource`), so a file lives while any tree built from it lives; its imports and images live while it does.
- **Stale ids** read `None` everywhere; the renderer skips them (or draws the checker in debug).
- **Not changed.** An image keeps its CPU pixels while loaded, as today, since atlas repacking and mipmaps rebuild from them.

## The asset types

- **`Image`** (today's `Sprite`, renamed).
  - Loads PNG, JPEG and the other raster formats the `image` crate is built with, and SVG. `ImageSettings` carries SVG's size (a scale, or a fit to a box).
  - `Image::decode(bytes)` turns embedded bytes into an image; there is no way to do that today.
  - The procedural constructors (`Image::solid_rect` and the rest) are kept, and their results go in with `add`.
  - `Renderable.sprite: SpriteHandle` becomes `Renderable.image: Handle<Image>`. `ParallaxLayer.sprite` and the other holders follow.
- **`Shader`** (`.wgsl`).
  - A material's `SpriteShader::Custom` and a custom post-process effect (`EffectsManager::add_custom`) hold a `Handle<Shader>`.
  - `Shader::from_wgsl(name, source)` covers embedded WGSL.
  - Compiling stays lazy in the renderer, with errors reported once per version.
- **`Font`** (`.ttf`, `.otf`, `.ttc`; the face index is a setting).
  - On load, its faces are registered with the text system; when freed, unregistered.
  - `TextFont::Id(FontId)` becomes `TextFont::Face(Handle<Font>)`, and `Fonts::set_role` takes a `Handle<Font>` and holds it. `TextFont::Role` and `TextFont::Family` are unchanged.
  - `Font::from_bytes(..)` with `add` replaces `Fonts::add_bytes`.
  - The built-in fonts are added at startup and held by the engine.
  - `Fonts` remains as the text system's face registry, fed by font assets, with no file loading of its own.
- **`BitmapFont`.**
  - Loaded from a `.bitmap.ron` descriptor (sheet path, cell size, characters), whose sheet image is a dependency.
  - Or made in code from a `Handle<Image>`.
  - A reloaded sheet redraws its text (4g's behaviour).
- **`UiFile`** (`.ui.ron`).
  - Its imports and the images it names are dependencies, loaded through the load context, which rejects import cycles.
  - `UiFile::parse(name, text)` with `add` replaces `add_ui` for embedded text.
- **`Theme`** (`.theme.ron`).
  - Its skin images are dependencies. Themes name fonts by family or role, so fonts are not file dependencies of a theme.
  - Its UI sounds stay plain paths handed to audio until 5b.
  - The UI theme follows a `Handle<Theme>` when it is set from one, which replaces `ThemeWatch`'s polling.
  - `Theme::save` stays.
- **Dependencies** are strong handles the parent asset holds, so they live while the parent lives. When a dependency changes, its dependents hear: a UI file's importers recheck, node trees built from it reconcile (4d), a theme refreshes its skins, and a bitmap font redraws.

## Hot reload

- **Watching.**
  - `DiskSource` watches the asset root with `notify`, debounced: a path reloads about 100 ms after its last event, so an editor's write-temp-and-rename reloads once.
  - It is on by default in debug builds; `EngineBuilder::hot_reload(bool)` overrides that.
- **Reloading.**
  - A change maps to every asset loaded from that path (each settings variant).
  - Each reload goes to the workers, which read, decode, and prepare what the render step would otherwise compute (such as mip levels).
  - The main thread only swaps the value in under the same id, bumps its content version and sends `Modified`; the renderer re-uploads on the next draw.
  - Assets loaded with `load_now` reload the same way.
- **Failures.** A file that no longer loads keeps the last good value and state `Loaded`, sends `ReloadFailed`, and warns once per broken content (as UI files do today). A deleted file keeps the last value.

## Public API and migration

- **Prelude.**
  - Added: `Handle`, `AssetId`, `Assets`, `AssetServer`, `Asset`, `AssetLoader`, `LoadContext`, `LoadState`, `AssetEvent`, `AssetError`, `Image`, `ImageSettings`, `Shader`, `Font`, `BitmapFont`, `UiFile`, `Theme`.
  - Removed, with no deprecation period:
    - `AssetManager`, `SpriteHandle`, `AsyncSpriteHandle`, `LoadingState`, `LoadingProgress`, `AssetStats`, `ShaderHandle`;
    - `Sprite::load`, `load_svg`, `load_svg_fit`, `Fonts::load`, `Fonts::add_bytes`, `Theme::load`, `create_sprite`, `cache_sprite_handle`, `gc_sprites`, `start_loading`;
    - the `AssetReloaded` and `AssetLoadFailed` messages.
- **Scene code** goes through `ctx.assets`:
  - `load`, `load_with` (settings), `load_now`;
  - `add`, `get`, `get_mut`;
  - `state`, `is_ready`, `progress`;
  - `stats` (per type: loaded, loading, failed).

  Systems use `Res<AssetServer>` and `Res<Assets<T>>`.
- **Migration, all in 5a:**
  - every game and demo, with the asset-loading demo rewritten around load states and progress;
  - the `ryo2d init` and `ryo2d dev` templates;
  - the module notes and `docs/architecture/overview.md`;
  - a new `ryo-assets` skill, and updates to ryo-api, ryo-text, ryo-ui, ryo-renderer and ryo-game-dev, with their CLI copies byte-identical. Also fix ryo-api's claim that dropping a `MusicHandle` stops the music.
- **Not touched.** Audio keeps its manager until 5b; `load_tiled_map` stays until the tilemap rewrite; saves, `system.json` and `Ryo.toml` are not assets.

## Acceptance

### Correctness

- **Unit tests, through `MemorySource`:**
  - ids, handles and stale reads; freeing on the last drop; a `load` racing a drop;
  - longest-extension loader matching; settings in the cache key;
  - load states and recursive readiness; the error variants and their `source()` chains;
  - `load_now` while a worker loads the same key;
  - every built-in loader: raster, SVG with settings, WGSL, TTF/OTF/TTC, `.bitmap.ron`, `.ui.ron` (imports, image dependencies, cycles), `.theme.ron`;
  - reload in place: same id, version bumped, dependents told, a broken reload keeping the last value.
- **Headless GPU tests:**
  - an image's texture or atlas region is freed when its last handle drops;
  - a freed font's glyphs leave the cache, and empty pages are freed;
  - a freed shader's pipelines are dropped;
  - a stale or failed image draws the checker in debug and nothing in release;
  - a reloaded image draws its new pixels.
- **Exactness:**
  - the demo tour is pixel-identical to main's. Anything a scene drew at once before is loaded with `load_now`, so no checker appears;
  - every UI pixel oracle stays green;
  - `tests/ui_cache_alloc.rs` stays at or below 14, so handles add no steady-state allocation;
  - the wasm build compiles.

### Targets

Measured alternately against main, with the machine's load reported:
- **Scene start.** 40 images, 2 fonts and 3 UI files loaded with `load_now`: no slower than today's synchronous loads. Through `load` and the workers: faster.
- **Idle and empty frames:** within 3% of main.
- **Reload hitch.** A 2048×2048 PNG and an SVG reloading while an engine runs: the main-thread time of the frame the new value lands in, plus the next render step, is within 0.5 ms of an ordinary frame's, upload included.

A missed target goes to the owner with a profile; no target is loosened without the owner.

## Documentation

- **Module notes:**
  - `src/core/asset/AGENTS.md`: the registry, handles and lifetimes, loaders and sources, the pump, hot reload;
  - text, UI, graphics and render notes where fonts, files, themes and textures meet the registry.
- **`docs/architecture/overview.md`:** assets and the frame.
- **Skills:**
  - the new `ryo-assets` (loading, states, progress, lifetimes, custom loaders, hot reload, embedding with `add`);
  - updates to ryo-api, ryo-text, ryo-ui, ryo-renderer and ryo-game-dev.

## Design decisions

- **Own registry, not `bevy_asset`.** The engine runs standalone bevy_ecs in its own loop. `bevy_asset` needs Bevy's App, plugin model and task pools. Its loader traits have changed across releases.
- **Drop-counted handles, not explicit release.** Explicit release leaks when forgotten and breaks when called early (Unity Addressables). Handles freed by `Drop`, applied in one place each frame, avoid both.
- **Ids, not handles, in engine caches.** If the renderer held handles (today's 600-frame clone), the engine could not free what the game let go.
- **A path and its settings form the key.** One file at two SVG sizes is two images. Hot reload updates both.
- **`load_now` exists.** Most game code loads in `enter` and draws at once; making it async would add a frame of checker or blank in every scene. Async `load` is there for loading screens and streaming.
- **The checker in debug only.** A missing image should be visible while developing, and invisible to players.

## Risks

- **The rename's reach.** `Sprite` → `Image` and `SpriteHandle` → `Handle<Image>` touch about 60 engine files and 160 example lines. Do the rename on its own, mechanically, before changing behaviour, so a tour run separates the two.
- **Upload cost.** A 2048×2048 image's upload alone may exceed the 0.5 ms reload target. If it does, the numbers go to the owner (chunked uploads are a possible lever).
- **Fonts in the text system.** Registering and unregistering faces must keep Parley's collection and the layout cache consistent. Text laid out with a freed font must not panic: it falls back as an unknown family does today.
- **Glyph pages are shared between fonts.** Freeing one font's glyphs can leave pages partly used; only an empty page is freed.
- **Pumping on wasm** within a frame budget must not stall the browser's frame on a large image.

## Out of scope

- Audio as an asset (5b).
- Embedded and web sources, and a release asset manifest (the next asset sub-project, on the source seam).
- Tilemap loading, until the tilemap rewrite.
- Asset processing or import caches.
- Dropping images' CPU pixels after upload.
- Saves, `system.json` and `Ryo.toml`.
