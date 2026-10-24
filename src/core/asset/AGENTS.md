# AGENTS.md - Asset Module

- `AssetManager` is a resource; scene callbacks get it as `ctx.assets` (taken out of the world for the callback), systems as `Res<AssetManager>` / `ResMut<AssetManager>`.
- `asset_pump_system` (`PreUpdate`, every frame) finishes async loads (`process_async_loads`) and, in debug builds, hot reloads (`process_reloads`), then writes `AssetReloaded` / `AssetLoadFailed` messages. Don't call either from game code; poll an `AsyncSpriteHandle` or `loading_progress()` instead.
- Hot reload is the registry's: with `EngineBuilder::hot_reload` on, its `DiskSource` watches the asset folder. The old manager's watcher is no longer started, since nothing it loaded is left.
- Loading the same path twice returns the cached handle; `SpriteHandle` wraps `Arc<RwLock<Sprite>>`, so hot reload updates every handle in place (the renderer re-uploads it: its atlas key includes `generation()`).
- `AssetManager` handles sprites only; audio has its own manager.
- `AssetManager` must stay `Send + Sync` to be a resource: its async-load and hot-reload channels are crossbeam channels, not `std::sync::mpsc`.
- **UI files** (`.ui.ron`) are registry assets: `UiFileLoader` (`loaders/ui_file.rs`) loads a file's imports and the images it names as its dependencies, and its `check` builds the root and every template against them; a reload rechecks the files that import it. A dependency that fails on the workers reaches the pump as the server's `Dependency` error, which a loader's `dependency_failed` may replace with a better one (the UI file loader names the position); the default keeps it. The UI module notes (Files) say how trees follow a file.
