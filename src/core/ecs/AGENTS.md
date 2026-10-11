# AGENTS.md - ECS Module

`World` wraps `hecs::World`. For pooling internals see skill `ryo-ecs-internals`; for tweens see `ryo-animation`.

## hecs Query Patterns

hecs 0.11: query iterators yield only the query item, not `(Entity, item)`. Older-style examples (including some skills) are wrong.
- Include `Entity` in the tuple to get IDs: `query::<(Entity, &T)>()`
- `query()` returns a `QueryBorrow` - call `.iter()`
- `query_mut()` returns an iterator directly

## PropertyTween Auto-Driving

- `TweenSystem` (`systems/tween.rs`, `update_property_tweens`) drives only `PropertyTween<f32>`, `<Vec2>`, `<Color>`
- Other `Lerp` types (incl. built-in `(f32, f32)`, `Rect`): call `tween.update(dt)` manually, or add a loop there

## Object Pooling

- Engine passes skip `Inactive` entities (rendering, physics, animation, tweens). User `render_gpu()` code walking the world must skip them itself (`gpu.world.has::<Inactive>(e)`) or iterate `pool.active_iter()`
- `acquire()` doesn't reset components - set every field you rely on
- Pools track `world.generation()`. After `world.clear()` (default scene Switch/Reset cleanup), the next pool call empties the pool; only the `Expand` policy refills it, `ReturnNone` keeps returning `None`
- `release_all()` resets a pool on level end/restart
