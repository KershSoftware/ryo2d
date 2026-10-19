# UI Render Retention (4g) Design

## Overview

Sub-project 4g of the professional-grade overhaul makes an unchanged UI nearly free to render, and a changing UI cost what changed. It sits on 4a (text and UI drawing), 4e (the cheap frame) and 4f (cached node trees): `docs/specs/2026-10-15-text-and-ui-drawing-design.md`, `2026-10-18-ui-fast-design.md`, `2026-10-18-ui-cache-design.md`.

What it adds:

- **A fast glyph lookup.** The glyph atlas's map moves to the engine's fast hasher.
- **Image retention.** When a frame's UI commands match last frame's, the renderer keeps last frame's UI image and skips building, uploading and drawing the UI.
- **Incremental build.** On a changing frame, a command built before reuses its built draw items; only new or changed commands are built.
- **Tooling:** `ui:retain on|off`, `EngineBuilder::ui_retain`, a render line in `debug:ui-rebuilds`, and `HeadlessEngine` counts.
- **A measurement at the end:** whether incremental upload and render bundles (approach C) are worth a follow-up.

**Why:** 4f made the UI pass incremental, but nobody had measured what happens to the UI's commands after it. A profile of the 200-widget settings node screen (headless, 1280x720, after 4f) gave, per frame:

| Part | Cost |
|---|---|
| UI pass (walk, replay, layout, resolve) | about 34 to 38 µs |
| `render::ui::emit_ui`, building draw items | 70.6 µs |
| of which the glyph atlas lookup, SipHash per glyph | about 30 µs |
| `Batch2D::prepare`, instance remap and upload (~150 KB) | 28 µs |
| one more render pass encoded (~145 draws) | about 19 µs |

The render side is about 120 µs, about 3.2 times the UI pass, and it is spent again every frame whether or not anything changed. Unity UI Toolkit keeps each element's geometry and Flutter keeps layers between frames; this sub-project gives Ryo2D's renderer the same property.

**Decisions taken with the owner:**

- **Purpose:** finish parity: idle UI near-free end to end. Layout and resolve retention for node trees follows as its own sub-project (4h).
- **Order:** render retention first: it is the larger cost, it serves both UI styles, and it is independent of the node cache.
- **Approach B:** fast lookup, image retention and incremental build. Incremental upload and render bundles (approach C) are left for a measured follow-up.

## The render path today

The UI pass leaves the frame's UI in `UiCommands`: `(order, UiCommand)` pairs, where a `UiCommand` is a `Rect`, `Sprite`, `Line` or `Text` in UI units, each with a clip and a layer (`src/core/ui/immediate/commands.rs`). Both UI styles and the absolute `draw_*` calls write it.

At present time (`src/core/engine/extract.rs`), `emit_ui` (`src/core/render/ui.rs`) turns each command into draw-list items: a rect into a shaped quad instance, a sprite into a sprite instance, a line into triangles, a text into one instance per glyph (and per outline, shadow or glow layer), each glyph looked up in the glyph atlas. Every item's depth is `ui_z(layer, order)`; its clip becomes a scissor. The whole draw list is sorted, `Batch2D::prepare` remaps and uploads every instance, and the `ui` pass draws the UI camera's items into the compositor's UI image (`Rgba8Unorm`, gamma, premultiplied), cleared every frame. The composite then blends the UI image over the frame.

The UI image is composited over the world, so a UI that did not change can be shown again as it is, whatever the world does.

## A fast glyph lookup

`GlyphAtlas`'s `entries: HashMap<GlyphKey, ShelfEntry>` (`src/core/graphics/glyph_atlas.rs`) uses std's SipHash. It moves to `FastMap` (`src/core/hash.rs`), as every other engine key did in 4e: the keys are the engine's own data. It is looked up for every glyph of every text, world text included, every frame. No behaviour change.

## Image retention

At the start of the render step each UI command gets a **key**: a fast hash of its value — geometry, colours, clip, layer, effects — with its shared text layout or line points identified by pointer, and its sprite by handle and version (below). The frame's **sequence** is its keys in painter's order.

When the sequence equals last frame's, nothing in it is time-driven, the UI's environment is unchanged and the compositor's UI image still holds last frame's UI, the renderer:

- builds no UI draw items and adds none to the draw list;
- leaves the UI out of the batch upload;
- skips the `ui` pass, which would clear the image;
- composites last frame's UI image, which is pixel-identical by construction.

The cost of an idle frame's render side is then the key comparison.

## Incremental build

On a changing frame, a command whose key has a **built entry** reuses it: its draw items are pushed again with the depth stamped from this frame's order (an earlier command's change can move it). A command without one is built as today, and its items are stored under its key. Entries not used in a frame are dropped at its end. The draw list, the sort, the batch and the pass then run as today.

- **Glyph slots are pinned.** A reused text's glyph instances hold atlas positions, so the slots an entry uses are kept from eviction while the entry is used: each frame it is reused, its slots are stamped by slot index, with no hashing.
- **Sprites need no pinning:** sprite atlas positions are remapped at batch time from the sprite handle, as now.

## What a key covers

A command's built items depend on more than its own value:

- **The UI environment:** `UiSpace` (pixel scale, image size), pixel-art snapping and the UI image's resolution mode. A change clears every entry and redraws the image.
- **The glyph atlas:** a font reload or an atlas reset clears every entry; the atlas gains a generation for it.
- **Text layouts and line points, by pointer.** An entry holds its own clone of the shared value, so its address cannot be freed and reused by different content while the entry exists.
- **Sprites, by handle and version:** a sprite hot-reloaded in place changes the key, even at the same size, and so redraws the image.
- **The clip** (the draw's scissor), the layer, colours, opacity and effects.
- **Time-driven drawing is never reused:** a text with a time-based effect (wave, shake, pulse and the like), a typewriter reveal, or anything else that reads the frame's time when built is built every frame, and while one is on screen the image is redrawn every frame.
- **Painter's order:** not part of a command's key (it is re-stamped); for keeping the image the whole sequence, in order, must match.
- **The image itself:** a UI image the compositor recreates (resize, device loss) is redrawn. A frame with no UI uses the composite's transparent placeholder as today, and the next frame with UI draws in full.

## Exposure and tooling

- **Automatic** for every UI: closure UI, node trees, absolute `draw_*` calls and the engine's overlays. There is no API change.
- **`ui:retain off|on`** (console) and `EngineBuilder::ui_retain(bool)` turn retention off. Off is exactly today's path, for comparing output and for ruling retention in or out when chasing a bug. It is separate from `ui:cache` (node-tree building).
- **`debug:ui-rebuilds`** gains a render line: "UI image: kept" or "redrawn", and "commands built N / reused M".
- **`HeadlessEngine`** gains render-side counts (image kept, commands built and reused), `#[doc(hidden)]`.

## Acceptance

### Correctness

- **The pixel oracle** is the main proof. Two `HeadlessEngine`s run the same script, retention on and off, and every frame's read-back image must be identical, pixel for pixel. Scripts cover:
  - every page of the `ui` demo (closure and node);
  - breakout's menus, HUD and play;
  - the `text` demo's effect pages (time-driven);
  - a HUD whose counter changes every frame;
  - hover and focus moves, scrolling, a dropdown and a modal opening and closing;
  - a resize and a theme change mid-run;
  - a sprite hot-reloaded in place, at the same size and at another;
  - a font reload;
  - a small glyph atlas forcing evictions while reused glyphs are on screen;
  - the UI hidden, then shown again.
- **Counts:** an idle frame keeps the image; one changed label builds exactly one command; a time-driven text is built every frame and the image redrawn; a scale or space change clears every entry; with retention off, everything is built.
- **Pixels:** the demo tour is identical to a baseline captured before 4g's first change (nondeterministic pages excluded, as in 4e and 4f).

### Benches

Headless with the GPU (`HeadlessEngine`), at 1280x720:

- An idle settings node screen adds **under 5 µs** of render-side work over an empty frame (about 120 µs today).
- The settings screen with one label changing every frame keeps the render side **under 60 µs**.
- `benches/text.rs`'s HUD frame drops by about the glyph lookup's cost.

## Documentation

- **`ryo-ui` skill:** an unchanged UI costs almost nothing to render; a time-driven text effect keeps the UI redrawing every frame; `ui:retain` and the render line in `debug:ui-rebuilds`.
- **`src/core/render/AGENTS.md`:** the keys, what they cover, the pinning and the image rule, with the reason for each.
- **Console help** and `ryo-api` for the switch and the counts.

## The end of 4g: measure again

With retention landed, profile the idle and the changing frames again, and write into `docs/backlog/ui-retained-nodes.md` whether incremental upload and render bundles (approach C) are worth a sub-project, with the numbers that decide it, alongside 4h (layout and resolve retention). The owner decides.

## Design decisions

- **Equivalence is the spec.** Retention must not be observable; the pixel oracle compares it on and off frame by frame.
- **Key by value, at the renderer.** The renderer sees only commands, so keying them serves both UI styles and needs nothing from the UI pass.
- **Keep the image, not just the geometry.** The UI is a composited layer: an unchanged UI is shown again without drawing it.
- **Pointer identity is held, not borrowed.** An entry keeps a clone of each shared value it is keyed by, so no address is reused under it.
- **Time-driven drawing is never cached.** Effects that read the clock are drawn as today.

## Risks

- **A missed input** shows a stale UI. Mitigated by the pixel oracle across the hard cases and by the switch.
- **Glyph eviction** while an entry is reused would corrupt text. Mitigated by pinning and an eviction test.
- **Key cost** on a large UI: the hash per command each frame; measured.
- **Memory** for built entries, about the size of one frame's UI items.

## Out of scope

- Incremental upload of changed instance ranges and recorded render bundles (approach C), left for a measured follow-up.
- Layout and resolve retention for node trees (4h).
- Dirty-rectangle partial redraws of the UI image.
- World (non-UI) rendering.
