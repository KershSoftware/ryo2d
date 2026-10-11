# AGENTS.md - Asset Module

- Engine enables hot reload in debug builds when the workspace has an assets dir, and calls `process_reloads()` every update - don't call it from game code
- Async loads (`load_sprite_async()`, `start_loading()`) only complete if the game calls `ctx.assets.process_async_loads()` each frame - the engine never does
- Loading the same path twice returns the cached handle; `SpriteHandle` wraps `Arc<RwLock<Sprite>>`, so hot reload updates every handle in place
- `AssetManager` handles sprites only; audio etc. have their own managers
