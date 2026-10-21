# UI Renderer (4h) Design

## Overview

Sub-project 4h of the professional-grade overhaul gives the UI a renderer of its own, so that a changing UI frame costs roughly what changed. It is "approach C" from 4g (`docs/specs/2026-10-19-ui-render-retention-design.md`, `docs/backlog/ui-retained-nodes.md`), reshaped by measurement. Layout and resolve retention for node trees, called 4h until now, moves to 4i.

What it adds:

- **A `UiRenderer`.** It turns the frame's `UiCommands` straight into GPU instances in painter's order. The world's batch, draw list, world text, trails and lighting are untouched.
- **One pipeline with clipping in the shader.** Rects, glyphs (raster, colour, MSDF), sprites and lines draw from one instance format through one pipeline. Each instance carries its clip rect, so a draw breaks only when a bound texture changes.
- **The UI's own instance buffer**, with one upload a frame.
- **4g's retention on top.** An unchanged UI keeps its image; on a changing frame, the text cache holds each text's finished instances, so a reused text is a single copy.

**Why.** 4g made an unchanged UI cost nothing to render, but a changing one still costs 81-96 µs of render-side CPU on the 200-widget settings node screen with one label changing every frame (4g's target was 60 µs). A profile of that frame (headless, 1280x720, Apple GPU) gave:

| Part | µs | What is in it |
|---|---|---|
| Encoding the UI pass | 23.2 | 64 draws; 62 of the 63 breaks are pipeline switches (rect, text, line) |
| `emit_ui` | 17.9 | mostly copying reused glyphs into the shared draw list (873 x 192 B); 59% of the items are culled later |
| `Batch2D::prepare` | 25.6 | placing items 8.7, uploading instances 11.4, uploading line vertices and indices 5.0 |
| `UiRetain::keep`, sort, submit | 7.2 | |
| Unattributed | about 12 | |

What each candidate change would remove (measured in micro-benchmarks, estimated in place):

- **One pipeline with shader clipping:** about 22-26 µs (64 draws become 1 or 2).
- **A direct UI path:** about 12-17 µs. Instances are written straight into the UI's own array, culled first, skipping the draw list, the sort and the placing.
- **Uploading only what changed:** about 12-14 µs, but only while instance positions are stable, and worse than a full upload beyond about 5 changed ranges.
- **Render bundles:** about 1.4 µs once there are about 5 draws. WebGPU bundles cannot set the scissor, and on native wgpu replays them draw by draw anyway.

**Decisions taken with the owner:**

- **Target:** one changing label under 40 µs of render-side CPU (estimated about 36 µs). Uploading only what changed stays a follow-up, measured after this lands.
- **Approach A: a dedicated UI renderer**, rather than patching the shared batch with an uber pipeline. That would keep the copying and placing, give about 60 µs, and miss the target.

## The UI renderer

```
UiCommands -> cull (outside clip or image) -> UiInstances in painter's order (one array)
           -> runs (break only when a bound texture changes) -> one upload
           -> the ui pass: one pipeline, one draw per run, no scissors
```

- The world keeps `Batch2D` and the draw list. The UI camera stops being one of the batch's cameras, and UI lines stop using the shared triangle stream.
- 4g's retention works on top. `UiRetain::keep` still decides whether the UI image can be kept; a kept frame runs none of this. On a changing frame `UiTextCache` stores each text's finished `UiInstance`s, with their glyph pages pinned and checked by generation as now.
- Both UI styles benefit: the renderer only sees commands.

## The instance and the shader

**`UiInstance`.** One instance format for every kind: rect, raster glyph, colour glyph, MSDF glyph, sprite (an sRGB page, or a render target's linear one), and line triangle. Each carries:

- a `kind`;
- its clip, as four integers: the scissor rect `scissor_of` gives today, or the whole UI image for no clip;
- the fields its kind reads today, laid out for the UI rather than overlaid on the world's 128-byte `SpriteInstance`.

Quads keep today's transform rows, so the vertex shader computes their corners with exactly today's arithmetic, pixel-art snap included.

**Lines.** A line is one instance per triangle of today's line builder (`Triangles::polyline`: solid style, miter joins, butt caps), each carrying its three corners' positions, uvs and colours; the quad's other three vertices repeat the last corner. Instances are the builder's triangles rather than segment quads and wedges so the geometry is exactly the builder's, triangle for triangle (amended after Task 3; first designed as one quad per segment and wedge).

**One pipeline.**

- **Vertex:** today's `vs_main` for quads, and a corner path for line triangles.
- **Fragment:** a switch over `kind` running today's seven fragment bodies unchanged (`fs_ui_rect`, `fs_text_mask`, `fs_text_color`, `fs_text_msdf`, the sprite body, the linear sprite body for a render target's page (`fs_base_linear`), the line body), each now a shared include function the world entries call too. Each then goes through the same `ui_out` with the `UI_GAMMA` override.
- **Sampling:** texture reads use explicit-gradient sampling (`textureSampleGrad`), as WGSL's uniformity rules require inside the switch. This samples identically: glyph pages have one mip level, and the sprite body already takes its own derivatives.

**Clipping in the shader.** The clip test runs last in the fragment: the pixel centre (`@builtin(position).xy`, at k + 0.5) must lie in `[x0, x1) x [y0, y1)` of the instance's integer clip, and pixels outside output zero. Under the UI's premultiplied blend, a zero output leaves the image as it was. This is exactly the scissor test, and it runs after all derivative-taking code.

**Textures.**

- One bind group holds the current raster-glyph page, colour-glyph page, MSDF page and sprite texture (an atlas page, a standalone image or a render target), plus the samplers today's pages use: the atlas's linear and nearest pair. Unused slots hold a 1x1 placeholder.
- An instance picks its slot by kind and its sampler by a flag.
- A draw breaks only when the next instance needs a different texture in its slot. The settings screen uses one glyph page, so it takes one draw.
- Bind groups are cached by the textures and samplers they hold. wgpu handles compare by identity, so a recreated page or texture is a new key; an entry is dropped once it has gone unused for 8 drawn frames, releasing the textures it holds (amended after Task 2; first designed as dropped when a texture is recreated).
- Four sampled textures and two samplers fit WebGPU's default limits, so this works on the web.
- The glyph atlas does not change. Texture arrays per format, which would need the atlas to grow by copying pages, are left for later if many pages interleave in practice.

## The frame

1. **Order.** Commands are walked in painter's order: layer, then recorded order. They usually arrive in that order, so the renderer checks once and sorts an index list only if one is out of place.
2. **Cull.** A command that draws nothing is skipped before anything is built: one wholly outside its clip or the UI image, or with an empty clip (as today). This removes exactly what today's scissor and camera culling remove, only earlier.
3. **Build.** Rects, sprites and line pieces are built into the instance array. Texts go through `UiTextCache`:
   - a reused text is one copy of its stored instances;
   - a changed text is built, then stored;
   - texts with effects, bitmap-font texts and texts cut short by a full atlas are built every frame, as today;
   - icons are built fresh, as today.
4. **Runs.** While building, a new run starts whenever an instance needs a different texture in its slot. A run is an instance range and its bind group.
5. **Upload.** The UI has its own instance buffer, grown by doubling, with one `write_buffer` of the frame's instances. It replaces the three shared writes (instances, line vertices, indices).
6. **The `ui` pass.** It clears the UI image, sets the pipeline once and binds the UI camera's uniform. Then, for each run, it binds that run's texture group and draws the range.

Draw counts and frame stats include the UI's draws. `ui:retain off` still means everything is built and drawn every frame.

## Acceptance

### Correctness

**Old against new, pixel for pixel.** While 4h is built, both UI paths exist behind a temporary, hidden engine switch. A pixel oracle (two `HeadlessEngine`s, as 4g's `PixelPair`) runs each scene with the old path and with the new renderer, and every frame must be identical. Scenes:

- every page of the `ui` demo, and breakout's menus, HUD and play;
- 4g's hard cases: named icons swapped, a sprite updated in place, a font render change (MSDF), bitmap fonts, text effects, glyph pressure from UI and world text, pixel art, a resize and a theme change mid-run;
- scroll clipping and nested clips;
- checkmarks, dropdown chevrons and `draw_line`;
- colour glyphs and a standalone (non-atlas) image;
- a UI using two glyph pages, forcing texture breaks.

A difference is a bug. The one exception is a difference proven to come from the GPU evaluating identical math differently; that is brought to the owner before merge.

**The old path is removed in the last task,** once the oracle passes everywhere. That covers the UI's route through the draw list and `Batch2D`, UI lines in the triangle stream, and the switch. 4g's retention oracle (on against off) stays, and the demo tour is compared with a baseline captured before 4h's first change.

**Counts:**

- the settings screen draws in at most 2 draws (1 expected);
- the two-page UI takes the expected number;
- a reused text is copied once;
- a culled command builds nothing.

### Benches

Headless with the GPU, at 1280x720, in the `ui_render` guard:

- One label changing every frame: **under 40 µs** of render-side CPU over an empty frame (asserted). If missed, the profile is reported and the owner decides, as with 4g's target.
- An idle settings screen: still under 5 µs.
- Retained no slower than unretained.

The HUD and scrolling benches are measured again.

## Documentation

- **`src/core/render/AGENTS.md`:** a "UI renderer" section, with the instance, the pipeline, clipping, textures, runs and the frame, each with its reason. It replaces the notes on the UI going through the batch.
- **`ryo-ui` skill:** the performance notes (the UI draws in one or a few draws; what breaks a run).
- **`docs/backlog/ui-retained-nodes.md`:** the measured numbers, and whether uploading only what changed is worth it.
- **`docs/backlog/follow-ups.md`:** every deferred finding, at merge.

## Design decisions

- **The UI is batched apart from the world.** The UI needs painter's order, clips, a few textures and no lighting; the world needs cameras, layers, lights and custom shaders. Mature engines keep these batches apart, and so does this one now.
- **Clipping moves into the shader.** Per-draw scissors forced a draw per clip, and they cannot be recorded in render bundles. The shader test is exactly the scissor.
- **Same math, moved.** Quads keep today's vertex arithmetic and fragment bodies, so pixel identity is a requirement rather than a hope.
- **Texture slots, not texture arrays.** The glyph atlas stays as it is; a run breaks on the rare texture change.
- **No render bundles.** Once draws are few, bundles save about 1 µs on native.

## Risks

- **Pixel differences from restructured shaders** (the switch, gradient sampling). Mitigated by the old-against-new oracle across all scenes.
- **Many interleaved glyph pages** (a UI with text at many sizes) would break runs often. This is counted, and texture arrays are the follow-up.
- **A larger instance format** costs upload bytes. It is measured, and the format is kept tight.
- **Web behaviour** (each wgpu call crosses into JS) is not measured here. Fewer calls should help more there.

## Decisions after review

- **UI lines on Apple GPUs (owner, 2026-10-20).** On Apple/Metal, the old path's indexed line mesh, whose triangles share vertices, gets a different `fwidth` near a shared edge than the same triangles drawn apart, as the UI renderer draws them. UI line pixels may therefore differ from the pre-4h renderer by that derivative class alone: up to 3/255 along slanted edges and up to about 0.36 coverage at miter tips and closed-outline corners; on the demo tour, only the text demo's page 5 differs, in 34 pixels at miter tips, by at most 67/255. Proven, not assumed: the new path equals the old line pipeline drawn without shared vertices (`lines_draw_as_the_batch_draws_them_with_no_vertex_shared`). Accepted. Next: analytic, derivative-free line anti-aliasing for UI and world lines, as a small sub-project of its own with a visual review (`docs/backlog/ui-retained-nodes.md`, 4h).
- **The measurement** (Task 8) is in `docs/backlog/ui-retained-nodes.md`, 4h, with the guard's caveat: its empty-frame baseline is paced by the GPU, so it reads the changing frame lower than other measures do.

## Out of scope

- Uploading only the changed instance ranges (a measured follow-up).
- Render bundles.
- Texture arrays for glyph pages.
- The world's batch.
- Layout and resolve retention for node trees (4i).
