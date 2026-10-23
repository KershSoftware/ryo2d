# UI Fragments (4i) Design

## Overview

Sub-project 4i of the professional-grade overhaul makes a node-tree UI's unchanged subtrees keep their layout and resolved output between frames, in the UI pass and in the UI renderer, so a frame costs about what changed. It is 4f's "approach B" (layout and paint retention, `docs/backlog/ui-retained-nodes.md`), built on 4h's renderer.

What it adds:

- **Fragments.** Every outermost clean subtree of a node tree keeps its layout, its records and its commands from the frame that resolved it. A later frame copies them, moved to where they land, instead of replaying, laying out and resolving the subtree again.
- **Whole-pixel scrolling with scroll slots.** A scroll area draws its contents moved by whole render pixels, and what it shows is placed relative to it, so scrolled content is unchanged output under a moving offset.
- **A fragment-aware renderer.** The renderer keeps each fragment's instances and copies them; the vertex shader moves a scroll slot's instances by the slot's offset, so a scrolling list's instances do not change.

**Why.** Measured on main `99e457cb` (Apple M5 Pro, Metal, 1280x720; the 200-widget settings node screen; research of 2026-10-21):

| Per frame, main thread | Idle | One closure label changing | Scrolling |
|---|---|---|---|
| UI pass (`SceneManager::ui_pass`) | 38.5 µs | 46.0 µs | 40.5 µs |
| replay (tape copies, ids) | 7.7 | 7.3 | 7.5 |
| dirty check | 2.7 | 3.0 | 3.4 |
| layout (the description walk and its hashes) | 6.0 | 6.5 | 6.4 |
| text (cache hits) | 3.0 | 3.1 | 2.6 |
| resolve (rects, clips, records, commands) | 15.5 | 15.7 | 16.1 |
| the closure label (reshaping "Frame N") | 0 | 6.9 | 0 |
| render side over an idle frame | 0 (kept) | +35.9 µs | +43.8 µs |

About 96% of the layout, text and resolve time in the label frame is spent on subtrees that did not change. Four things make that so:

- A root with a scroll area is rebuilt every frame (scroll areas are live).
- Layout and resolve run per root, over every node.
- Draw orders come from one counter for the whole pass.
- Records are rebuilt from nothing each frame.

While a list scrolls, every row moves by a fractional offset, so the renderer rebuilds and uploads nearly every instance (`build_ui` 12.5 µs, `finish` 13.5 µs).

If clean subtrees cost nothing, an unchanged frame's pass would fall from 38.5 µs to about 6 to 8 µs, and a focus move every frame from 36 to about 9.

**Decisions taken with the owner:**

- **Purpose:** parity with retained UI engines (Flutter, Unity UI Toolkit), so that cost follows what changed, with measured targets.
- **Scope:** both the UI pass and the renderer.
- **Scrolling:** scroll offsets draw in whole render pixels (a visual change, reviewed by eye).
- **Approach A:** a fragment at every outermost clean subtree, rather than fixed boundaries (roots and scroll contents) or a persistent per-node tree.
- **Structure:** one spec in two parts: the pass, then the renderer.

## Part 1: fragments in the UI pass

### What a fragment is

In the node walk (`nodes/draw.rs`), a node that replays (clean by 4f's rules, `take_clean`) under a parent that rebuilt becomes a fragment, as does a clean root. A fragment is therefore the outermost clean subtree under the dirty path. 4f guarantees that a clean node's descendants are clean.

These take today's replay path instead:

- a subtree whose tape holds popup ops (`PopupBegin`/`PopupEnd`: dialogs, open lists, context menus), since popups are flushed at the scene's end with fresh orders;
- live nodes (scroll areas, lists, unfinished typewriters). These are dirty every frame, so no fragment contains one.

A fragment inside an embedded tree (`ui.node` in closure UI) works the same way: it joins the closure's subtree as one node.

### What it keeps

From the frame that resolved it, a fragment keeps:

- **Its layout.** Its subtree stays in the layout engine's Taffy tree, untouched. The layout engine keeps its descendants' entries alive without walking them: the end-of-frame sweep (`LayoutEngine::end_frame`) must not remove them. A text leaf in it can still be measured when Taffy lays the subtree out again (the measure callback finds it through the fragment).
- **Its inputs from outside.** These are:
  - its laid-out size and its position relative to its slot;
  - the available size and constraints it was laid out under;
  - its clip, layer, scene and top-scene flag;
  - the focus scope and scroll ancestor it was stamped with;
  - the `UiEnv` stamp and the tape's reads, as 4f's record.
- **Its resolved output, relative to its slot.** Positions are relative to its scroll slot (below; slot 0 for content under no scroll area):
  - Its records.
  - Its commands, in the order resolve made them. Orders and depths are kept relative to its first order and its root's depth.
  - Its commands unculled, so a fragment that moves or is scrolled culls again exactly.
- **What resolving it again needs.** Its stamped node descriptions, its draws and Taffy's relative positions for its nodes.

### Each frame

A fragment's node enters its parent's subtree as one node: its descendants are not replayed. Its parent and the dirty path are laid out as today. Taffy reuses the fragment's subtree results when its constraints are unchanged, and re-lays it when they are not. Then one of three things happens:

1. **Same size, same inputs, same position relative to its slot.** Its output is copied. Orders and depths are shifted to its base this frame, rects are made absolute by its slot's offset for the records, and commands are culled (below).
2. **Same size and inputs, moved by whole render pixels within its slot** (a sibling grew by whole pixels). The same copy, with its rects translated.
3. **Anything else:** moved by a fraction of a pixel, resized, constraints changed, or an input differs. It is resolved again from what it kept, against Taffy's current layout, and that output is kept for next frame. This skips the replay and the layout walk; Taffy re-lays only what changed.

In all three cases a fragment also does what a replay does today:

- re-registers its ids (`replay_id`) and touches its widget states;
- carries its records into the new frame's memory without resolving them;
- counts in the walk's statistics: built, replayed, and now reused.

**Draw order.** The pass's order counter is shared by everything drawn in a frame. A fragment's orders form one contiguous range, so it reserves its count from the counter where the rebuild would have taken them, and its commands and records are shifted by its base. A rebuild in its place would assign exactly those values.

**Rerun passes.** A UI pass rerun (a scene switched from `ui`) and `UiHarness`'s rewound frames reuse the fragments of the last committed frame, never those of a discarded pass. Fragments are double-buffered like records.

**Dropped:** a fragment is dropped when its node rebuilds, is despawned, leaves its scene, or the env changes, and when it is not drawn in a frame.

### Exactness

A frame with fragments must equal the same frame without them:

- **records:** bit for bit;
- **commands:** after culling, bit for bit, in the same order;
- **widget states, interaction, focus memory and events:** equal;
- **pixels:** identical.

Any input that can change a fragment's output without dirtying its node is either checked (its inputs above) or excluded (popups, live nodes). In debug builds the spot check (4f's, extended) resolves one reused fragment from scratch each frame and compares; a difference warns once and names the fragment's node.

## Whole-pixel scrolling and scroll slots

### The rule

A scroll area's `ScrollState.offset` keeps its meaning:

- the wheel eases it, the trackpad moves it at once (1:1 with the fingers), and `scroll_to`, clamping and the scrollbar read it as today;
- the offset it is **drawn** at is rounded to whole render pixels: `round(offset × px_scale) / px_scale`, per axis.

What a scroll area shows is placed relative to the area's snapped rect, which lies on whole pixels, then moved by the drawn offset.

**At rest, nothing changes.** At a whole-pixel offset, snapping relative to a whole-pixel origin gives the same edges as snapping absolute positions. The exception is an edge within float error of a half pixel. The demo tour must match its baseline; any difference is brought to the owner.

**While content moves, it moves rigidly.** Each rect edge no longer rounds on its own (today a row's height can change by a pixel mid-scroll). Content resting at a fractional trackpad offset lies at most one pixel from where it lies today. The scrollbar thumb is not content and keeps using the exact offset.

### Slots

Every scroll area is a slot for what it shows; slot 0 is content under no scroll area.

- **Its data each frame:** a slot holds its pixel offset (its snapped origin less its drawn offset, in whole render pixels, absolute) and its clip (its view, intersected with the clips around it).
- **Nesting:** slots compose. A slot's origin lies in its parent slot, so the Clipping page's card strip moves with the list around it and scrolls on its own.
- **Its content:** commands and records under a scroll area carry its slot, with positions relative to it. Within a slot, the clip of everything it shows is the slot's clip, as only scroll areas clip their children. (Correction: other nodes clip their children too, a text field's view among them, so inside a slot a node under one keeps a clip of its own, relative to the slot, met with the slot's where it is drawn.)
- **Nodes placed with `at`:** such a node inside a scroll area does not scroll (as today). It is placed in the area's parent slot and keeps the area's clip as its own.
- **Records stay absolute.** A record's rect is its slot-relative rect plus its slot's offset, so hit tests, navigation, `scroll_to`, popups anchored to a widget and every widget reading a rect read what was drawn.
- **Text** under a slot is placed relative to the slot in render pixels. A whole-pixel move along x keeps each glyph's subpixel bin, so a horizontally scrolled text is reused too (closing 4h's follow-up).

### Culling against the slot's interest rect

The renderer receives fragment runs and loose commands, and they are culled differently:

- **Loose commands** (the dirty path, closure UI, overlays, popups) are culled to their clip exactly as today, so closure UI sends the renderer what it sends now.
- **A fragment's run under a slot** is culled against the slot's interest rect: the slot's view grown by one view along each axis it scrolls, with the grown rect's position stepped in half views, keyed by the slot's offset. A fragment's run under slot 0 is culled to its clip, which does not move.

So a scrolling list's fragment runs change only every half view of travel. Content that scrolls into view is already in the renderer's hands. A long static scroll area still sends a bounded amount.

The oracles compare commands after culling both sides to their clips, so the interest rect is not a difference.

## Part 2: the renderer

### Fragment runs in the command stream

A fragment's commands reach `UiCommands` as one tagged run. Its tag holds:

- the fragment's id (its node entity);
- a version that changes whenever its output is resolved again (case 3) or its culled run changes (its interest rect moved);
- its order base, its layer and its slot.

Because its orders are contiguous and on one layer, a run is contiguous in painter's order. Loose commands stay as they are; each carries its slot.

### Keeping the UI image (4g)

`UiRetain::keep` compares:

- a fragment run by id, version, base, layer and slot;
- loose commands as today;
- the slots' offsets and clips, as part of what the image depends on.

A frame whose only change is a slot's offset is not kept (its pixels moved), but nothing in it is built.

### Instances per fragment (4h)

The renderer keeps each fragment run's built instances, positioned relative to its slot:

- **A reused run:** its instances are copied into the frame's array.
- **A new or changed run:** built through today's path (`build_command`, the text cache), then stored.

A run's instances are checked the way the text cache checks a text: glyph-page generations (an evicted page drops them), sprite and icon stamps, and the UI image's size, scale and presentation. A run not drawn in a frame is dropped.

### Slots in the shader

`ui.wgsl` reads a per-frame uniform array of slots. Each slot holds its pixel offset and clip, and there are up to 256 per frame. The array is a uniform within WebGPU's default limits.

- **Instances:** an instance names its slot in spare bits of its flags (`meta.w`), and the vertex shader adds its slot's whole-pixel offset to its corners. An instance in slot 0 keeps its own clip; one in another slot draws under its slot's clip.
- **Overflow:** beyond 256 slots, the content of the extra scroll areas is built absolute, as today.
- **Culling:** the vertex shader collapses a quad that lies wholly outside its clip, so unculled fragment instances cost no fill. The fragment shader's clip test stays the exact scissor test, so pixels are unchanged.

### Upload

The frame's instance array is assembled as today, from copied runs and built loose commands, and 4h's changed-range upload stays:

- an idle frame, a scrolling frame between interest steps and a closure label changing write nearly nothing;
- a fragment rebuilt mid-tree writes from it to the end of the array, as a changed text does in 4h.

Stable per-fragment ranges in the buffer would need more draws; they stay a measured follow-up.

## Acceptance

### Switches

`ui:fragments on|off` (console), `EngineBuilder::ui_fragments(bool)` and a `UiHarness` setter. Off is today's replay path under the new scrolling rule. The oracles run fragments on against off.

### Correctness

- **The node oracle** (`nodes/oracle.rs`, 21 scenarios) runs fragments on against off: records, culled commands, states, interaction and events, frame by frame. It gains a scenario for each input a fragment depends on, each failing without that input's check (4f's rule):
  - a sibling inserted before a fragment (its order base moves);
  - a sibling whose text grows (the fragment moves by a fraction, then by whole pixels);
  - a window resize;
  - a clip, layer or top-scene change, and a scene pushed over the tree;
  - each env change;
  - nested scroll slots, scrolled by wheel and trackpad;
  - a popup opened next to a fragment;
  - an embedded tree in closure UI;
  - virtual-list rows;
  - a rerun UI pass.
- **The pixel oracle** (`PixelPair`, fragments on against off) covers:
  - every ui demo page and breakout's menus, HUD and play, frame by frame, with wheel and trackpad scrolling at UI scales 1, 1.25 and 2;
  - render cases: glyph-page eviction, a sprite updated in place, a font change, and more than 256 slots.
- **4f's and 4g's oracles** (cache on and off, retention on and off) and their tests keep passing.
- **The spot check** runs with fragments on in debug builds.
- **The demo tour** matches a baseline captured before 4i's first change. Static pages rest at whole offsets, so a difference is a bug or an edge case brought to the owner.
- **The scrolling review.** Before-and-after recordings of the same wheel and trackpad gestures on Scroll Views, Clipping & Modals, Big list and the settings screen, for the owner to accept by eye.
- **Allocations:** `tests/ui_cache_alloc.rs`'s unchanged cached frame allocates no more than today (14).

### Targets

On this machine, the settings node screen unless named. Each target is asserted in a guard. A target that is missed is reported with a profile, and the owner decides, as in 4g and 4h.

| Frame | Today | Target |
|---|---|---|
| `benches/ui` node screen, unchanged | 32.0 µs | under 15 µs |
| focus moving every frame (a new bench) | 36.1 µs | under 18 µs |
| a 2,000-node static tree | 303 µs | under 100 µs |
| the closure settings screen | 49.0 µs | no slower (within 5%) |
| engine UI pass, idle (a new `HeadlessEngine` reading of the pass's time) | 38.5 µs | under 18 µs |
| engine UI pass, one closure label (6.9 µs of it is the label's own shaping) | 46.0 µs | under 25 µs |
| engine UI pass, scrolling | 40.5 µs | under 20 µs |
| render step over empty, one label (`benches/ui_render.rs`) | about 33 µs | under 25 µs |
| render step over empty, scrolling | about 37 µs | under 20 µs |
| render step over empty, idle | about 1 µs | still under 5 µs |

The `debug:ui-rebuilds` overlay counts fragments reused, resolved again and built, beside 4f's rebuilt and replayed nodes.

## Documentation

- **`src/core/ui/AGENTS.md`:**
  - a "Fragments" section: the unit, what it keeps, the three cases, the inputs it checks and why, draw order and records, the spot check;
  - "Scrolling" rewritten for whole-pixel offsets, slots and interest rects.
- **`src/core/render/AGENTS.md`:** fragment runs, instance reuse and its checks, slots in the shader, vertex-stage culling.
- **`ryo-ui` and `ryo-renderer` skills:** performance notes (cost follows what changed; what makes a subtree rebuild), whole-pixel scrolling, and the `ui:fragments` switch.
- **`docs/backlog/ui-retained-nodes.md`:** the measured numbers.
- **`docs/backlog/follow-ups.md`:** every deferred finding, at merge.

## Design decisions

- **The boundary is found, not declared.** 4f's dirty tracking already knows which subtrees are clean, so every outermost clean subtree is a fragment. A game marks nothing, and a hover in a list re-resolves that row, not the list.
- **Output is kept relative to its slot.** Scrolling moves a slot, not its content, so the pass and the renderer reuse scrolled content unchanged, as browsers' and Unity UI Toolkit's scroll views do.
- **Whole-pixel offsets make that exact.** With content and offsets on the pixel grid, a moved slot's content equals a rebuild. The owner accepted the visual change.
- **Same output, kept or rebuilt.** A fragment's output is defined as what a rebuild would produce, so the oracles compare on against off and a difference is a bug.
- **Popups and live nodes stay out.** They depend on things outside their subtree (the order counter at the scene's end, anchors, per-frame state); they are few, and they are rebuilt each frame already.

## Risks

- **An input a fragment does not check.** Mitigated by a scenario per input, the spot check and the pixel oracle on every page.
- **Layout engine state.** A fragment's Taffy subtree kept between frames, the end-of-frame sweep, and text measured inside a fragment after a resize. Covered by the resize and growing-sibling scenarios.
- **Bookkeeping that costs more than it saves** on small UIs that change every frame. Guarded by the closure screen and the focus-moving bench.
- **Memory.** Fragments hold resolved output, instances and text layouts. They are bounded by what is drawn and dropped when not drawn.
- **Float edges in relative snapping** at rest, caught by the tour.
- **The web** (the slot uniform, vertex culling) is not measured here.

## Out of scope

- Retaining closure UI, which stays immediate by design.
- Stable per-fragment GPU ranges and uploading several ranges.
- Render bundles and texture arrays.
- Closure text reshaping (a closure label shapes its text every frame).
- Analytic line anti-aliasing (its own sub-project).
