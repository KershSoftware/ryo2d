# GPU World Rendering - Design

## Overview

Every world element - entity sprites, shadows, tiles, particles, parallax, `render_gpu` sprites, trails and lines - is drawn on the GPU from one z-sorted draw list, so they share a single z-order. The CPU entity renderer (the scene pixel buffer, its upload and the per-pixel sprite rasterizer for entities) is retired. This is renderer sub-project 2 (owner decisions, 2026-10-10): **z-interleaved layering, done by moving CPU sprites to the GPU, in one sub-project that lands together**, with a texture atlas.

**Why:** today GPU content (particles, parallax, `render_gpu`) is drawn before the CPU pixel buffer is blended over it, so it always sits behind every entity; `z_order` only orders GPU batches among themselves (and a batch spanning several z values draws as one block); trails and lines ignore z (`Line2D::z_order` unused, `Trail2D` has none); `CompositeTile` ignores its `z_order`. Games already fight this (leap-king's rain at "above background, below platforms" isn't honoured). The CPU path also uploads the whole frame every frame (3.7 MB at 720p, 8.3 MB at 1080p).

**Owner decisions (2026-10-10):** move entity rendering to the GPU; one sub-project landing together; equal-z ties draw **entities first**, then `render_gpu`/particle/parallax sprites, then trails and lines; add a texture atlas; breaking demos and games is allowed (fix, don't shim).

**Out of scope:** custom WGSL sprite shaders (`SpriteShader::Custom`, sub-project 3); the immediate-mode UI (stays CPU-drawn into its own layer, composited last); the raycast path; lighting changes (it still lights everything the world pass draws).

## Architecture

- **`DrawList`** (crate-internal, rebuilt each frame): items with a sort key `(z: i32, kind, sequence)`:
  - `kind` order at equal z: `Entity` (incl. its shadows, tiles, composite parts) < `Sprite` (`render_gpu`, particles, parallax) < `Trail` (trails and lines).
  - `sequence` keeps submission order within a kind (entities in render-cache order, which already includes YSort's `z × 10000 + y` key; a shadow is submitted just before its entity).
  - Payload: a sprite instance (texture key + `SpriteInstance` + effects flag) or a trail/line group (a vertex range in the trail renderer, lit or unlit).
- **`world` pass** draws the sorted lit items into the game texture in **one render pass**: uploads first, then it walks the list, merging consecutive sprite items that share an atlas page and effects flag into one instanced draw and drawing trail ranges in between (pipeline switches inside the pass). **`unlit_world`** does the same for unlit trails/lines and the debug overlays after lighting.
- **Built-in passes** (`default_2d`): `clear`, `raycast`, `world`, `lighting`, `unlit_world`, `transition`, `post_process`, `ui`, `composite`, `capture`. `upload`, `gpu_sprites`, `cpu_blend`, `trails` and `unlit_trails` are removed (`pass_ids` updated).
- **Entities** are collected on the CPU as today (render cache, `Inactive`/`visible`, `WorldTransform`, interpolation, pivot, offset, YSort depth scale, camera `world_to_screen` with shake and zoom, culling) and emitted as screen-space `SpriteInstance`s - the format particles already use. Positions are snapped to whole pixels exactly as the CPU path's `as i32` did.
- **Texture atlas** (`graphics/atlas.rs`): 2048×2048 `Rgba8Unorm` pages (raw sRGB bytes) allocated with `etagere` (fallback: an in-house shelf packer if the crate can't be fetched); each entry has a 1-pixel extruded border; images larger than 256 px on a side get their own texture. Entries are keyed by sprite handle identity + content generation (entity sprites, so hot-reloaded or edited sprites re-upload) or by texture id (`render_gpu`/particle/parallax textures). Entries unused for 300 frames are freed when a page is full; then a new page is added (up to 8), then the least recently used entries are evicted. Callers keep texture ids and normalized UVs; UVs are remapped to the entry at draw time.
- **Sprite shader:** samples the `Rgba8Unorm` atlas (sRGB bytes), applies tint and effects in sRGB space like the CPU did, and converts to linear on output (the game texture stays `Rgba8UnormSrgb`). `SpriteInstance` gains `uv_bounds` (the sprite's own rect): effect samples outside it are transparent, and effects that grow the sprite (outline, glow, directional outline) draw on a quad enlarged by their padding. `DirectionalOutline` is added (the only CPU effect missing on the GPU).
- **Shadows:** `DropShadow` = the sprite drawn as a silhouette in the shadow colour, offset and scaled (blur: offset copies, as the CPU did); `BlobShadow`/`ContactShadow` = a cached solid-ellipse atlas texture scaled to the shadow size.
- **Tiled / CompositeTile:** one instance per tile; `CompositeTile` parts use the composite's `z_order` (fixes it ignoring z).
- **Trails/lines:** `Trail2D` gains `z_order` (default 0, builder `with_z_order`); `Line2D::z_order` is honoured. The trail renderer groups geometry by z and draws a group on request inside the world/unlit render pass.
- **Debug overlays** (colliders, velocities, origins) become lines in `unlit_world`, on top and unlit. **Transition fade** becomes the `transition` pass: a full-screen quad of the overlay colour × alpha after `unlit_world` (so it now covers trails too); the UI stays above it as today.
- **Stats:** `DebugStats.draw_calls` counts the world/unlit instanced draws and trail draws.

## Fidelity

Opaque pixels of unrotated sprites at whole-pixel positions and integer scales (including tint) match the CPU exactly. Translucent overlaps blend in linear space (the CPU blended entity-over-entity in sRGB bytes), so they may differ slightly; rotated or fractionally scaled sprites may differ at edges; effects need not match the CPU's pixels but must visibly do the same thing.

## Error handling

Atlas exhaustion degrades (evict LRU, then draw the item from a standalone texture) rather than failing; a texture that can't be created (zero-size sprite) is skipped. No `unwrap`/`panic` in library code.

## Testing

- **Atlas (no GPU):** allocation, extrusion layout, keying by handle + generation, LRU eviction order, large-image bypass.
- **Migration:** the GPU world path is built next to the CPU path behind a crate-internal switch; headless comparisons require exact frames for opaque, axis-aligned content (sprites at several z, tiled, composite, drop shadow without blur, tinted opaque sprites); then the CPU entity path is deleted.
- **Lasting (headless pixel probes):** a particle between two entities at intermediate z; a trail between two entities; equal-z ties (entity < particle < trail); YSort order; `CompositeTile` honouring z; a shadow under its entity; flip, tint and rotation probes; each built-in effect visibly applies (outline pixels appear outside the sprite); no atlas bleeding under 3× scaling; a runtime-generated sprite changing content re-uploads; the transition covers trails but not the UI; debug collider outlines draw; 10,000 sprites from a few textures render in at most a handful of draw calls (`DebugStats.draw_calls`).

## Documentation

Skills (renderer, ECS internals, particles, lighting, game-dev; both copies, CRLF), `src/core/graphics/AGENTS.md`, `src/core/render/AGENTS.md`, `src/core/trail/AGENTS.md`, rustdoc. A list of demos/games for the owner to look over by eye; demos relying on "GPU content is always behind" get explicit z values.

## Design decisions

- **GPU entities over a CPU depth layer:** exact ordering including translucency, no full-frame upload, and it unlocks custom shaders later.
- **One render pass for the world:** interleaving needs pipeline switches between sprite runs and trail groups; one pass avoids dozens of pass begin/ends.
- **sRGB-space shading:** keeps tints and effects looking like they do today.
- **Atlas behind texture ids:** `render_gpu` and particle code keeps its texture ids and UVs; only the renderer knows about pages.
