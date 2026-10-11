# AGENTS.md - Graphics Module

## Rendering Paths

```
CPU path:  Sprite.draw() → pixel buffer → WgpuRenderer → screen
GPU path:  SpriteInstance → BatchCollector → SpriteGpuRenderer → GPU
```

**When to use the GPU path:** 1000+ sprites (particles, bullets), per-sprite shader effects, or when CPU blitting becomes the bottleneck. Most games use the CPU path. GPU is opt-in via `render_gpu()`.

**Compositing order** (the `default_2d` render pipeline in `core/render/`):
1. `render_gpu()` sprites (incl. parallax) draw into the game texture first.
2. The CPU pixel buffer (cleared to transparent each frame) is alpha-blended on top.
3. Trails, lighting, post-processing, then UI follow.

So GPU sprites always sit **behind** CPU-rendered entities. `z_order` only orders GPU batches among themselves.

## GPU Sprite Batching

- `BatchCollector` groups instances by `BatchKey` (texture ID + has-effects). Each batch's z is the **minimum** `z_order` of its instances, and batches are sorted by it. Within a batch, instances draw in insertion order.
- **Synchronization**: all instance data is written to the instance buffer **before** the render pass begins, so the GPU never reads a buffer still being written. Each batch gets its own offset, and the pass binds `instance_buffer.slice(offset..end)`.

## Parallax

- A builder-driven system (`GpuParallaxBackground`), not ECS components: layers are one cohesive visual, and render-time tiling is cheaper than spawning duplicate entities.
- Rendered from `render_gpu()`, so it draws behind all CPU-rendered game entities.

## Camera Shake

- Trauma-based. Offset = smooth noise × `max_offset` × `intensity_curve(trauma)`, where the default curve `Ease::InQuad` gives trauma².
- The engine calls `update_shake()` with **unscaled** dt, so shake keeps working during slow-mo/hitstop.
- The offset is applied automatically in `world_to_screen()`. `screen_to_world()` does NOT remove it.

## Notes for AI Agents

1. **Camera is separate from rendering**: it only handles coordinate transforms.
2. **Animation updates automatically**: `systems/animation.rs` updates `Animator`/`AnimationController` each frame.
