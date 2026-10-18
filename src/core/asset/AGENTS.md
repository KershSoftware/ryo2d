# AGENTS.md - Asset Module

- `AssetManager` is a resource; scene callbacks get it as `ctx.assets` (taken out of the world for the callback), systems as `Res<AssetManager>` / `ResMut<AssetManager>`.
- `asset_pump_system` (`PreUpdate`, every frame) finishes async loads (`process_async_loads`) and, in debug builds, hot reloads (`process_reloads`), then writes `AssetReloaded` / `AssetLoadFailed` messages. Don't call either from game code; poll an `AsyncSpriteHandle` or `loading_progress()` instead.
- The engine enables hot reload in debug builds when the workspace has an assets dir (never headless).
- Loading the same path twice returns the cached handle; `SpriteHandle` wraps `Arc<RwLock<Sprite>>`, so hot reload updates every handle in place (the renderer re-uploads it: its atlas key includes `generation()`).
- `AssetManager` handles sprites only; audio has its own manager.
- `AssetManager` must stay `Send + Sync` to be a resource: its async-load and hot-reload channels are crossbeam channels, not `std::sync::mpsc`.
- **UI files** (`.ui.ron`, `src/core/ui/file/`) are cached by resolved path like sprites, in `ui_files`:
  - `load_ui` reads, checks and caches a file; its imports load through `AssetManager` as the `ImportResolver`, so they are cached too, and `ui_loading` stops an import cycle. All of a file's mistakes come back together as `AssetError::UiFile`.
  - `add_ui(path, text)` checks text the game embedded and caches it under `path`, so later loads and imports use it. An embedded file is never watched or reloaded (`reload_ui` returns Ok and does nothing).
  - `reload_ui(path)` checks the file's text again and, on success, replaces its document in place (`UiFile::replace` moves the generation), then `recheck_importers` checks every cached file that imports it, directly or through others, against its own text. A file that no longer checks keeps its old contents with a warning. The trees built from a file follow it through `reload_trees` (see the UI notes).
  - `ron` is in the default hot-reload extensions, so the watcher also sees `.ui.ron` files. `reload_ui_from_event` skips an event whose text equals the loaded text, and warns once per broken text: `ui_warned` keeps a hash of the last broken text per file, cleared by a clean reload and by a save back to the loaded text (so the same mistake saved again after a revert warns again). Outcomes go to the `AssetReloaded` / `AssetLoadFailed` messages like other assets.
  - Release builds never watch; the files are read once.
