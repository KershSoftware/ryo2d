# AGENTS.md - Asset Module

- `AssetManager` is a resource; scene callbacks get it as `ctx.assets` (taken out of the world for the callback), systems as `Res<AssetManager>` / `ResMut<AssetManager>`.
- `asset_pump_system` (`PreUpdate`, every frame) finishes async loads (`process_async_loads`) and, in debug builds, hot reloads (`process_reloads`), then writes `AssetReloaded` / `AssetLoadFailed` messages. Don't call either from game code; poll an `AsyncSpriteHandle` or `loading_progress()` instead.
- The engine enables hot reload in debug builds when the workspace has an assets dir (never headless).
- Loading the same path twice returns the cached handle; `SpriteHandle` wraps `Arc<RwLock<Sprite>>`, so hot reload updates every handle in place (the renderer re-uploads it: its atlas key includes `generation()`).
- `AssetManager` handles sprites only; audio has its own manager.
- `AssetManager` must stay `Send + Sync` to be a resource: its async-load and hot-reload channels are crossbeam channels, not `std::sync::mpsc`.
