# UI Renderer (4h) Design

## Overview

Sub-project 4h of the professional-grade overhaul gives the UI a renderer of its own, so that a changing UI frame costs roughly what changed. It is "approach C" from 4g (`docs/specs/2026-10-19-ui-render-retention-design.md`, `docs/backlog/ui-retained-nodes.md`), reshaped by measurement. Layout and resolve retention for node trees, called 4h until now, moves to 4i.

What it adds:

- **A `UiRenderer`.** It turns the frame's `UiCommands` straight into GPU instances in painter's order. The world's batch, draw list, world text, trails and lighting are untouched.
- **One pipeline with clipping in the shader.** Rects, glyphs (raster, colour, MSDF), sprites and lines draw from one instance format through one pipeline. Each instance carries its clip rect, so a draw breaks only when a bound texture changes.
- **The UI's own instance buffer**, with one upload a frame (amended after Task 10: of only the range that changed since the last upload; see "Decisions after review").
- **4g's retention on top.** An unchanged UI keeps its image; on a changing frame, the text cache holds each text's finished instances, so a reused text is a single copy (amended after Task 11: a text that moved is laid again from its kept glyphs; see "Decisions after review").

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

- **Target:** one changing label under 40 µs of render-side CPU (estimated about 36 µs). Uploading only what changed stays a follow-up, measured after this lands (amended after Task 10: it was brought into 4h, and the target is met with it; see "Decisions after review").
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

- **Vertex:** today's `vs_main` arithmetic for quads, one include function both call (`quad_corner`, amended after the branch review), and a corner path for line triangles.
- **Fragment:** a switch over `kind` running today's seven fragment bodies unchanged (`fs_ui_rect`, `fs_text_mask`, `fs_text_color`, `fs_text_msdf`, the sprite body, the linear sprite body for a render target's page (`fs_base_linear`), the line body), each now a shared include function the world entries call too. Each then goes through the same `ui_out` with the `UI_GAMMA` override.
- **Sampling:** texture reads use explicit-gradient sampling (`textureSampleGrad`), as WGSL's uniformity rules require inside the switch. This samples identically: glyph pages have one mip level, and the sprite body already takes its own derivatives.

**Clipping in the shader.** The clip test runs last in the fragment: the pixel centre (`@builtin(position).xy`, at k + 0.5) must lie in `[x0, x1) x [y0, y1)` of the instance's integer clip, and pixels outside output zero. Under the UI's premultiplied blend, a zero output leaves the image as it was. This is exactly the scissor test, and it runs after all derivative-taking code.

**Textures.**

- One bind group holds the current raster-glyph page, colour-glyph page, MSDF page and sprite texture (an atlas page, a standalone image or a render target), plus the samplers today's pages use: the atlas's linear and nearest pair. Unused slots hold a 1x1 placeholder.
- An instance picks its slot by kind and its sampler by a flag.
- A draw breaks only when the next instance needs a different texture in its slot. The settings screen uses one glyph page, so it takes one draw.
- Bind groups are cached by the textures and samplers they hold. wgpu handles compare by identity, so a recreated page or texture is a new key; an entry is dropped once it has gone unused for 8 frames whose UI was drawn (all of it culled counts), when a frame keeps the UI image and the image was drawn without it, or when a frame's UI records nothing, releasing the textures it holds (amended after Task 2 and the branch review; first designed as dropped when a texture is recreated).
- Four sampled textures and two samplers fit WebGPU's default limits, so this works on the web.
- The glyph atlas does not change. Texture arrays per format, which would need the atlas to grow by copying pages, are left for later if many pages interleave in practice.

## The frame

1. **Order.** Commands are walked in painter's order: layer, then recorded order. They usually arrive in that order, so the renderer checks once and sorts an index list only if one is out of place.
2. **Cull.** A command that draws nothing is skipped before anything is built: one wholly outside its clip or the UI image, or with an empty clip (as today). This removes exactly what today's scissor and camera culling remove, only earlier.
3. **Build.** Rects, sprites and line pieces are built into the instance array. Texts go through `UiTextCache`:
   - a reused text is one copy of its stored instances (amended after Task 11: one drawn elsewhere is laid again from its kept glyphs, by the text emitter's own placing);
   - a changed text is built, then stored;
   - texts with effects, bitmap-font texts and texts cut short by a full atlas are built every frame, as today;
   - icons are built fresh, as today.
4. **Runs.** While building, a new run starts whenever an instance needs a different texture in its slot. A run is an instance range and its bind group.
5. **Upload.** The UI has its own instance buffer, grown by doubling, with one `write_buffer` of the frame's instances (amended after Task 10: of only the range that differs from the last upload, compared bit for bit; a new or grown buffer is given the whole frame). It replaces the three shared writes (instances, line vertices, indices).
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
- **The guard measures without the GPU's pacing (owner, 2026-10-21; Task 9).** Task 8's guard took each engine's frames alone, and a lone engine is paced by the GPU: a frame takes the GPU about 210 to 310 µs here, longer than a render step, so a step waits on the backlog, an empty frame most. Its empty baseline read high, and one label read 0 to 26.5 µs over it. The guard now steps its seven engines in turn, one frame each (every other round in reverse), waits for the GPU to finish all submitted work before each step (outside the measure), and takes each median over the same 2,000 rounds. Wall time then equals the render step's thread CPU time to within 0.5 µs. The wait also runs wgpu's cleanup of finished submissions, which a 60 Hz frame does in its next submit, so the guard reads a little low.
- **The targets are read on the screen they were set on (Task 9).** The shared settings fixture's list never scrolled, so Task 9 made it fill the panel (`settings_tree`); that screen draws fewer rows (355 instances against 461) and flattered the label reading. The idle and one-label targets are read on the original layout (`settings_tree_overflowing`), the scrolling frame on the new one. There, at Task 9, one label read +42.7 to +46.0 µs over an empty frame: the 40 µs target was missed.
- **Uploading only what changed came into 4h (owner, 2026-10-21; Task 10).** The upload was then the largest part of a changing frame (19.4 µs: a new staging buffer for every `write_buffer`, and the copy into it). The renderer keeps a copy of what its instance buffer holds and writes only the one range that differs, compared bit for bit from both ends (from the first instance that differs to the last when the counts are equal, to the end when they are not; the whole frame into a new or grown buffer). The changing label writes 1 to 3 of 461 instances. One label now reads +31.2 to +33.6 µs over an empty frame (before: +40.7 to +42.4), so the 40 µs target is **met and asserted**, with the idle target and retained-no-slower. The margin needs a quiet machine: at a load of about 4 one label read +39.1 to +40.2 µs. The numbers and the profile after it are in `docs/backlog/ui-retained-nodes.md`, 4h.
- **A scrolled text reuses the text cache (Task 11).** Once the settings list scrolled (Task 9), a retained frame cost 6 to 9 µs more than an unretained one: the cache keyed a text by its origin, so a moving list's texts missed every frame. Translating kept instances is not exact (each line's baseline rounds on its own, and a snapped centre is divided by the UI scale), so the cache keys a text without its origin, keeps each glyph's seed (its quad and image before placement) and lays a moved text again through the emitter's own placing, after checking along x that no glyph's subpixel bin changes. Scrolling retained now reads +36.3 to +37.2 µs over an empty frame against +42.8 to +43.4 unretained (it was +51.0 to +52.4), and the guard asserts that it is no slower retained. The numbers are in the backlog, 4h.

- **Bind groups go with the UI that needed them (branch review).** The cache aged only on frames whose UI built instances, so after the UI went away (no commands), held still (kept) or was all culled, its last groups, and the textures in them, stayed alive indefinitely: a title screen's full-screen background held through a gameplay scene with no UI, even once the sprite renderer had let it go. Now a frame whose UI records nothing drops every group, a kept frame keeps only the groups its image was drawn with (the ones a change most likely needs again), and a finished frame ages the cache even when all of it is culled. Making a group again took about 1 µs (measured: 4 views and the group, on Metal), so dropping one a later change needs is harmless.
- **Quads are placed by one function (branch review).** `vs_ui` had a copy of `sprite.wgsl`'s quad arithmetic, pixel-art snap included, which nothing kept in step. The arithmetic moved verbatim into `quad_corner` (`include/quad.wgsl`), which both vertex stages call, and a test fails if either body places quads itself. Nothing moved: the demo tour is byte-identical to the baseline outside the nondeterministic pages, but for the 34 accepted line pixels of the text demo's page 5.

## Out of scope

- Uploading only the changed instance ranges (a measured follow-up; brought into 4h after Task 8, see "Decisions after review").
- Render bundles.
- Texture arrays for glyph pages.
- The world's batch.
- Layout and resolve retention for node trees (4i).
