# Retained UI Nodes (Sub-projects 4c, 4d, 4e, 4f)

Noted at the end of sub-project 4b (UI interaction, `docs/specs/2026-10-16-ui-interaction-design.md`). The owner split the work into three sub-projects, each with its own spec and plan.

## 4c: Entity Nodes (done)

Spec: `docs/specs/2026-10-17-ui-nodes-design.md`.

What landed:
- `UiNode` trees in the closure API's names, spawned with `ctx.spawn_ui` as one entity per node. Each frame they are drawn through 4b's builders, keyed by entity (approach A).
- Public state components (`UiLabel`, `UiChecked`, `UiValue`, `UiIndex`, `UiSlot`, ...) that the game writes and the walk writes back on a change. `UiEvent` messages, also triggered on the node for observers.
- Roots owned by their scene. Node modals, `UiNode::confirm` and popups, with Back. Embedding in closure UI with `ui.node(root, world)`. Reusable components as functions.
- Node versions of the `ui` demo's four acceptance pages, the Mixed page, and breakout's menus.
- Faster UI ids for both styles: a word-wise folded-multiply hash in place of byte-wise FNV and SipHash.

The guard bench (`benches/ui.rs`): the 200-widget settings screen as a node tree runs at **112 µs** a frame, against **104 µs** for the same screen as a closure (about 8% more; the guard is 10%). That is 4e's baseline.

Measured, not done: the UI memory's own `UiId`-keyed maps (`UiMemory`'s records, states and focus memory, and the layout engine's entries) still use SipHash. Moving them to the id hasher made the closure screen 89 µs and the node screen 99 µs in a trial. That is faster for both, but the node screen comes out 10.4 to 11.6% slower than the closure, over the 4c guard. The same absolute walk cost is a larger share of a faster frame. Take it with 4e, which removes most of the walk's per-frame cost.

## 4d: UI Files and Templates (done)

Spec: `docs/specs/2026-10-17-ui-files-design.md`.

What landed:
- `*.ui.ron` files that load into the same `UiNode`, through our own RON reader, one vocabulary table shared with `UiNode`, and errors with line, column and a suggested name. `AssetManager::load_ui`, `add_ui` (embedded text) and `reload_ui`; `ctx.spawn_ui_file` and `ctx.ui_template`.
- Templates with parameters and defaults (also node parameters), usable from files and from code (`UiArgs`), and imports between files.
- Named bindings: `{name}` patterns in labels and tooltips, `bind` on value widgets, `ctx.ui_set` / `ui_get`, values kept per root. They write the same state components 4c uses.
- File node keys and the reconcile: `ctx.update_ui` and hot reload bring a spawned tree in line in place. Entities, ids, focus, scroll and the game's and the player's values survive; `UiReloaded` tells the game a root was reloaded.
- Hot reload through the asset watcher (`.ui.ron` in debug builds), `ryo2d check-ui`, and `UiHarness` support.
- The `ui` demo's node pages (menu, settings, inventory, dialogue, Mixed) and breakout's menus and HUD load from files.

Benchmarks (`benches/ui.rs`):
- An unchanged `ctx.update_ui` of the 200-widget settings tree takes **39.0 µs**, against **125.4 µs** to spawn it.
- The 4c guard, the node settings frame against the closure one, is about **+8.5 to +8.8%** after the bindings sync was added (Task 8). That is inside the 10% limit, but with less room than 4c's 8%.

Left for later:
- Rust templates callable from files (an unknown template name could fall back to a registry of functions).
- Loops and conditionals in files.
- Localisation tables.
- Patterns in a slider's, progress bar's or rebind row's `label` (they are literal kind options today).
- Building a binding pattern from a template parameter (`{{` stays an escape, so a pattern is passed in whole as an argument).
- A visual editor and a JSON schema for files.
- Animations and transitions in files.

## 4e: Fast Big UIs (done)

Spec: `docs/specs/2026-10-18-ui-fast-design.md`.

What landed:
- The cheap frame, profile first: a word-wise hasher for the engine's own keys (SipHash had been about 40% of a frame), text cache keys, reused buffers in place of per-frame allocation, a layout diff that skips unchanged nodes, cheaper looks, a leaner node walk, and the hot-reload query moved out of the world.
- Virtual lists in both styles, recording only the rows in view inside the scroll area (so clipping, wheel, scrollbar and scroll-into-view are shared), ids keyed by item index, rows bound by message. The `ui` demo has a Big list page.

Numbers (`cargo bench --bench ui`, load about 2.5):

| Bench | Before 4e | After 4e |
|--|--|--|
| closure settings screen | 100.1 µs | 52.3 µs |
| node settings screen | 116.0 µs | 60.2 µs |
| 1,000-row scroll list | 597.3 µs | 325.0 µs |
| 10,000-row virtual list (closure) | - | 7.6 µs |
| 100-row virtual list (closure) | - | 7.6 µs |
| 10,000-row node list | - | 8.6 µs |
| empty frame | 0.18 µs | 0.188 µs |
| `update_ui`, settings tree unchanged | 39.0 µs | 38.8 µs |
| spawn, settings tree | 125.4 µs | 122.1 µs |

The virtual lists cost the same for 100 or 10,000 items. The node screen is 7.9 µs (15%) over the closure one.

The new profile (xctrace Time Profiler, 10 s each; exclusive time charged to the nearest engine function, so hashing and allocation land on their callers):

| Bucket | Node settings frame | Closure settings frame |
|--|--|--|
| (a) node walk (`draw_node`, component reads, `update_lists`, bindings sync) | 8.5 µs (14%) | 0.1 µs |
| (b) builders the walk drives (widgets, containers, ids, command push) | 28.1 µs (47%) | 29.3 µs (56%) |
| (c) layout (taffy and the diff) | 10.6 µs (18%) | 10.7 µs (20%) |
| (d) text (shaping, cache keys and lookups) | 9.5 µs (16%) | 8.9 µs (17%) |
| (e) records, input, navigation, memory | 3.0 µs (5%) | 2.9 µs (6%) |
| (f) other | 0.5 µs (1%) | 0.4 µs (1%) |
| total | 60.2 µs | 52.3 µs |

Hashing is now about 6% of the node frame (it was about 40%); allocation about 10% (it was 7 to 12%). The node walk's own cost is the 7.9 µs difference between the two columns. Subtree caching could skip (a) and (b), at most 36.6 µs (61%) of the node frame, and that is a ceiling: `finish_subtree` and `push_node`, the biggest builder costs, record the tree that layout and drawing read, so a cached subtree would have to replay or share its recorded nodes and commands. It would not remove layout above the subtree or the text lookups unless those are cached too.

What subtree caching would have to track to be correct (all under `src/core/ui/`):
- Component changes on any node in the subtree: the public state components (`UiLabel`, `UiChecked`, `UiValue`, `UiIndex`, `UiOptions`, `UiTextValue`, `UiSlot`, `UiTooltip`, `UiHidden`, `UiDisabled`, `UiImage`; `nodes/components.rs`) and the `UiNodeKind` itself; reconcile (`nodes/reconcile.rs`) writes only on a change, so bevy change ticks could drive it, as `Changed<..>` already does in `nodes/bindings.rs:988`.
- Interaction state, per frame: hover, press, focus, drag (`memory.rs:146` `Interaction`; `interact.rs:46` `resolve_pointer`; `nav.rs:563` `resolve_focus`; `nodes/draw.rs:424` writes `UiInteraction`). The looks follow it, so any subtree under the pointer or focus is dirty each frame it changes.
- Animation and clocks: text reveal and typewriter (`immediate/context.rs`, `fade` at 203), the tooltip delay (`widgets/tooltip.rs:32`), the scroll ease (`widgets/scroll.rs:56`). Looks themselves are chosen by state, not tweened.
- Theme changes (`theme/mod.rs:134` `Theme`, read through `immediate/context.rs:477` and `nodes/draw.rs:548`, `1328`): any change dirties everything.
- Bindings: `ui_set` values per root and patterns in labels (`nodes/bindings.rs:530` `set`, `546` `store`, `654` `sync_root`); a changed name dirties every widget that uses it.
- Layout above the subtree: the available size and the parent's flow (`layout/engine.rs:149` `layout`), including the screen size (`nodes/list.rs:278` `update_lists` takes it).
- Popups and tooltips opened from inside (`popup.rs:77` `begin_popup`, `widgets/tooltip.rs`): they escape the subtree's clip and order.
- Scroll offsets and list views (`widgets/scroll.rs`, `nodes/list.rs`): a scrolled area moves its rows each frame it eases, and a virtual list rebuilds its rows by index.

**Recommendation: not now.** The 200-widget node screen costs 60 µs, under 1% of a 60 Hz frame and under 1% of a 120 Hz one, and the walk itself is only 7.9 µs of that. Caching's ceiling is 36.6 µs, and a real frame gets less: the subtrees under the pointer or the focus, any that scroll or animate, and every subtree after a theme or binding change still rebuild. To earn that, caching would need all eight kinds of tracking above, plus replaying recorded nodes and commands for layout, drawing and navigation (navigation reads last frame's records of every focusable). Each missed case is a stale-UI bug, the hardest kind for a game author to see. The case that motivated caching, very long screens, is already solved by virtual lists: 10,000 rows cost 8.6 µs a frame as nodes. Revisit when a real game screen measures its UI pass above about 1 ms, or a profile shows unchanged subtrees' builders above half of its frame. Even then, look first at cheaper steps: skipping hidden and off-screen subtrees, and reusing a static leaf's draw commands.

Left for later:
- Measured rows (variable height) in virtual lists.
- Horizontal virtual lists.
- Node subtree caching: done in 4f, below.
- The dropdown and choice lists virtualised (they stay short).
- Touch flinging and momentum.
- A node's label drawn without a per-frame copy (`UiLabel` as a shared value): the walk still copies each `UiLabel` into its text every frame.

## 4f: Cached Node Trees (done)

Spec: `docs/specs/2026-10-18-ui-cache-design.md`.

What landed:
- The tape (`ui/tape.rs`): what a node's builder pushed (nodes, draws, ids, state and live-rule registrations, the reads it made) is recorded relative to the node, and replayed anywhere, with the context fields stamped at the splice point. Records live in `UiNodes` (`nodes/cache.rs`), not in components, so the game's archetypes are unchanged.
- Dirty tracking: one generation for what every builder reads (theme, scale, font, prompt), the tracked components by change tick and removal cursor, bindings, list rows, hover/press/focus and the clocks (reveals, tooltips); a dirty node rebuilds its own calls, so does every ancestor, and a replay never resumes inside a half-open builder.
- Splice or build in the node walk (`nodes/draw.rs`): a clean node replays its record; embedded trees (`ui.node`) and virtual-list rows do the same.
- The spot check (debug builds): each frame one clean record is built in place of its replay and compared op by op; a difference warns once and means an input the cache does not track.
- The on/off oracle (`nodes/oracle.rs`, 21 scenarios, about 7,400 frames, two seeded random runs, again with the spot check on) and the ui demo's and breakout's `tests/cache.rs`, which compare a cached engine with an uncached one frame by frame; `tests/e2e/ui_cache.rs` for a pushed scene.
- Tooling: `debug:ui-rebuilds` outlines rebuilt nodes with a rebuilt/replayed counter, `ui:cache on|off` and `EngineBuilder::ui_cache` switch it, and `UiHarness` has `set_ui_cache`, `set_ui_spot_check` and `ui_cache_stats`. `tests/ui_cache_alloc.rs` holds an unchanged cached frame at 15 allocations (an uncached one: about 190).
- Optimisations found by profiling the cached frame: a replay clones each node straight into the subtree and shares its record's tape, a taped node keeps its style's hash, a clean node's record is looked up once, a text leaf lives in its subtree, a shape hash that folds a dozen words, one layout entry lookup per node, scroll state read only under a scroll area, a replayed draw pushed without resolving its prompts again, focus navigation sorting references to last frame's records, a layout frame end that skips its sweep when every node was laid out, and a placed line that shares its points.

Numbers (`cargo bench --bench ui`, load about 3; "before" is the cached frame as first landed, or the end of 4e where there was no cached frame):

| Bench | Before | After 4f |
|--|--|--|
| node settings screen, unchanged (3 built, 274 replayed) | 50.2 µs (about 60 at the end of 4e) | 33.7 µs |
| node settings screen, focus moving | 50.8 µs | 33.6 µs |
| closure settings screen | 54.5 µs (52.3 at the end of 4e) | 48.9 µs |
| 2,000-node static tree | 452 µs | 314 µs (0.157 µs per replayed node) |
| embedded node tree in closure UI | 4.52 µs | 3.81 µs |
| dirty check alone, settings tree | - | 1.35 µs |
| 10,000-row node list | 8.71 µs | 7.28 µs |
| 10,000-row virtual list (closure) | 7.6 µs | 7.46 µs |
| 1,000-row scroll list | 325 µs | 322 µs |
| empty frame | 0.188 µs | 0.270 µs |
| `update_ui`, settings tree unchanged | 38.8 µs | 42.0 µs |

The unchanged node screen is now 14.8 µs (30%) under the closure one, where it was 7.9 µs over. The empty frame and `update_ui` rows moved up about 40% and 8% against 4e's figures, on a different load and with the cache's per-frame work (the dirty check, a few clears) in each; the empty frame is 0.27 µs either way.

The new profile (xctrace Time Profiler, 10 s each; exclusive time charged to the nearest engine function from the leaf up, so hashing and allocation land on their callers). The replay and dirty check buckets are new; 4e's walk, builders, layout and text carry over, and its records, input, navigation and memory bucket splits into (g) and (h):

| Bucket | Node unchanged | Node, focus moving | 2,000-node tree | Closure settings |
|--|--|--|--|--|
| (a) walk (`draw_node`, component reads, bindings sync) | 1.35 µs (4%) | 1.71 µs (5%) | 0.5 µs (0%) | 0.1 µs (0%) |
| (b) builders run (rebuilt nodes, the root's close, command clear) | 2.03 µs (6%) | 1.75 µs (5%) | 11.2 µs (4%) | 18.1 µs (37%) |
| (c) replay (tape replay, node copies, id re-registration) | 8.53 µs (25%) | 8.10 µs (24%) | 72.8 µs (23%) | - |
| (d) dirty check (`refresh_dirty`, `mark_live`, `clean`) | 2.05 µs (6%) | 2.31 µs (7%) | 21.6 µs (7%) | - |
| (e) layout (taffy and the diff) | 5.04 µs (15%) | 5.01 µs (15%) | 34.8 µs (11%) | 7.5 µs (15%) |
| (f) text (shaping, cache keys and lookups) | 3.84 µs (11%) | 4.05 µs (12%) | 62.0 µs (20%) | 11.0 µs (23%) |
| (g) resolve, records, commands (`finish_subtree`, `push_node`, `tree`) | 9.23 µs (27%) | 8.98 µs (27%) | 103.0 µs (33%) | 9.9 µs (20%) |
| (h) input, navigation, memory | 1.32 µs (4%) | 1.42 µs (4%) | 7.5 µs (2%) | 2.1 µs (4%) |
| (i) other | 0.30 µs (1%) | 0.27 µs (1%) | 0.6 µs (0%) | 0.3 µs (1%) |
| total | 33.7 µs | 33.6 µs | 313.9 µs | 48.9 µs |

Against 4e's node screen (60.2 µs): the walk and the builders (a)+(b), 36.6 µs there, are 3.4 µs now in the unchanged frame (the 3 nodes built and the root's own work), and the cost moved to replay and the dirty check (10.6 µs together) and to what the tapes still feed: layout, text and resolve are about as they were (10.6, 9.5 and 3.0+ µs against 5.0, 3.8 and 9.2 µs; resolve now carries the work 4e filed under builders' `finish_subtree` and `push_node`). Moving the focus changes the buckets by under 0.5 µs: a focus move rebuilds only the nodes it touches, as the oracle expects.

Approach B's data. In the unchanged frame, layout, text and resolve (e)+(f)+(g) are 18.1 µs (54% of the frame): that is the ceiling for retaining layout and paint, and it is smaller than it looks:
- Layout (5.0 µs, 15%) is already the hash diff. Taffy computes nothing in an unchanged frame (under 0.01 µs of its samples); the 5 µs is `try_layout`'s walk of the descriptions, the style and flow hashes and one index lookup per node (`layout/engine.rs:186`, the entry compare at 270-285). B would skip this walk for clean subtrees, but a clean subtree's description still has to be known to its parent's layout.
- Text (3.8 µs, 11%) is cache hits: the keyed lookup (`TextEngine::layout_keyed`, 2.0 µs) and the text commands built at resolve (`tree::text_command`, 3.8 µs inclusive). There is no shaping; B would save the hit and the command, not a layout.
- Resolve (9.2 µs, 27%) is the largest piece and the real prize: `finish_subtree` (`immediate/context.rs:1185`, 7.5 µs) turns the replayed nodes into rects, clips, draws and records every frame (`tree.rs:288` `resolve`).
- Replay (8.5 µs) would shrink too if B reused records: the node copies (2.8 µs) and the id set (1.2 µs) exist to feed layout and resolve; but B cannot avoid the dirty check (2.05 µs), the walk (1.35 µs), input and navigation (1.3 µs), which are about 4.7 µs of floor, and tapes for what does rebuild.
- So a clean frame could fall from 33.7 µs to about 15 µs at best (an unchanged frame's floor is (a)+(b)+(c)+(d)+(h)+(i) = 15.6 µs, less whatever replay B's records also absorb, down to about 12.5 µs), roughly 2x. A real frame gets less: a hover or a focus move dirties the nodes under it and every node whose looks follow them.

What B would have to track to reuse resolved records and commands (all in `src/core/ui/`):
- Position: records and commands are absolute and snapped to render pixels (`tree.rs:410` `snap_rect`, `579` `snap`), and the snapping is by position, so a subtree that moves by a fraction of a pixel changes its edges: B would reuse only for whole-pixel shifts or re-snap.
- Clips: each node's clip comes from its ancestors' (`tree.rs:365-395`, `clips[p]`, `nodes[p].clips`), so a moved or resized scroll area changes every clipped descendant's draws.
- Scroll shifts: a scroll area's contents move by its state's offset (`tree.rs:383-390`); a subtree under one moves every frame it eases, and its records carry the shift.
- Order and layer: draws take `layer` and `order` from the painter's order (`tree.rs:78-79`, `184-186`, `454`), so inserting or removing a sibling before a clean subtree renumbers it.
- Text layout width: text is wrapped at the room left in its node's content box (`tree.rs:219-233`, the wrap slack at 22), so a change of the parent's width above a clean subtree re-lays its text.
- Layout inputs from above: the available size and the parent's flow (`layout/engine.rs:161`), the focus scope, and the `at` rect of absolutely placed nodes (`tree.rs:340`).

**Recommendation: not next; revisit with a retained UI renderer.** 4f already gives node trees the property that defines retained UI: building costs what changed. An unchanged 200-widget node screen is 33.7 µs, 30% under the same screen as closure UI and about 0.2% of a 60 Hz frame. Retaining layout and paint could take that to about 15 µs at best, and a real frame gets less, since every hover or focus move dirties what it touches. To earn those 18 µs, B needs six more kinds of tracking (pixel snapping by position, clip chains, scroll shifts, painter's order, text wrap width, layout inputs from above) inside `tree::resolve`, which every closure UI also runs. The oracle makes that far safer than it was when 4e weighed caching, but the gain is small for the surface it touches. Two cheaper steps come first if UI time ever matters: replay is now the largest bucket (8.5 µs), and bulk copies plus less id-set work could take a few µs from it; and resolve's per-node work (9.2 µs). Paint retention pays when the renderer keeps a subtree's geometry on the GPU across frames rather than rebuilding `UiCommands` every frame, so B belongs with a retained UI renderer, or with a game whose UI pass measures above about 1 ms. For full parity with Unity UI Toolkit and Flutter, B is the remaining half; the owner decides.

Left for later:
- Layout and paint retention (approach B), pending the owner's decision above.
- Closure UI is immediate by design and is not cached.
- Sharing records between identical subtrees, and building on several threads.
- Text with icons after an in-place sprite resize (found in 4f, in the text engine; under Left for Later below).

## 4g: UI render retention (measured)

The renderer keeps the UI image while the UI's commands and render inputs are unchanged, and reuses an unchanged text's glyphs when the UI changes. Measured on an Apple GPU (Metal), 1280x720, with `benches/ui_render.rs` (the 200-widget settings node screen; "render step" is `HeadlessEngine::render_time()`, the CPU side of building, uploading and encoding, not the UI pass). The machine is shared, so numbers move 10 to 15% between runs.

**The guard** (medians of 2,000 frames, over an empty frame's render step of about 88 µs; four runs):

| Frame | Retention on | Retention off | Target |
|---|---|---|---|
| idle settings screen | 0 ns (kept) | 117 to 129 µs | under 5 µs: met |
| one label changing every frame | 81 to 96 µs | 112 to 136 µs | under 60 µs: **not met** |
| changing frame, on against off | on is 31 to 40 µs faster | | on no slower than off: met |

The research measured the render side of this screen at about 120 µs (profile, `emit_ui` 70.6 + `Batch2D::prepare` 28.0 + about 19 for the extra pass); idle is now zero and a changing frame is about 80 to 95 µs, so the 60 µs one-label target is **not met**. Approach B's own share of that frame is about 21 µs (`emit_ui` 18.4 + `keep` 2.4); the rest is the shared batch and encode path that the spec left to approach C. The guard asserts the idle and retained-no-slower targets and reports the one-label target.

**Criterion** (`cargo bench --bench ui_render`, whole `step()`):

| Bench | Retained | Not retained |
|---|---|---|
| `settings_idle` | 122.7 µs | 254.8 µs |
| `settings_label` | 224.8 µs | 259.7 µs |
| `settings_scrolling` | 216.1 µs | 249.9 µs |
| `empty_frame` | 187.1 µs | |

`empty_frame` is GPU-paced (its fixed passes cost more than the CPU work, so `step()` waits on the GPU), which is why an idle retained screen steps faster than it. Compare the render-step medians above, not the whole-step figures, against an empty frame. A scrolling list rebuilds its visible texts' placement each frame (the glyphs are reused, the instances are rewritten); retained is about 34 µs faster. Reusing the instances by translation is the possible follow-up. (The wheel needs the pointer over a row: at the panel's centre it does not reach the scroll area and the bench would measure a kept frame.)

**`benches/text.rs`** (HUD of 100 texts and 50 panels, 640x360): 248.3 µs before 4g (research); 219.2 µs after the glyph-lookup fix alone (Task 1, before retention existed); 197.2 µs retained [194.6 199.9] and 227.2 µs with `ui_retain(false)`. 1,000 cached UI texts: 136.1 µs (626.8 µs in the research); 200 world labels: 146.6 µs.

**Profile of the changing frame** (`settings_label`, retained; Time Profiler, 40,000 frames, inclusive per frame, against an empty scene at 102.3 µs; step total 218.5 µs):

| Function | Per frame | Empty |
|---|---|---|
| `SceneManager::ui_pass` | 43.6 µs | 0.6 |
| `render::ui::emit_ui` | 18.4 µs | 0 |
| `UiRetain::keep` | 2.4 µs | 0 |
| text built (one label: `emit_text` and `push_glyph`) | 0.6 µs | 0 |
| `Batch2D::prepare` | 27.0 µs | 0.1 |
| `Queue::write_buffer` (all) | 28.2 µs | 14.5 |
| `WgpuRenderer::render_frame` | 100.6 µs | 77.0 |
| of which `encode_render_pass` | 65.8 µs | 45.4 |
| `CameraUniforms::sync` | 9.0 µs | 10.4 |

140 texts are reused and one built each frame. What `emit_ui` still does is copy the reused texts' instances (the cloned-iterator copies are about 12 µs of its 18.4); the hashing and quad building are gone.

**Approach C** (upload only the changed instance ranges; record the UI's draws once as a render bundle). Render-side work over an empty frame in a changing frame is about 69 µs: `emit_ui` 18, `Batch2D::prepare` 27 (its uploads about 14 more than empty), and the extra pass about 24 (`encode_render_pass` +20). So prepare, uploads and encoding are about three quarters of what is left, and the per-call costs (one staging buffer per `write_buffer`, about 150 draws encoded) dominate their size. A bundle of the unchanged draws plus a small pass for the changed ones could remove most of the encoding and upload, perhaps 25 to 35 µs, which would bring this frame under 60 µs, but it needs stable instance ranges across frames (the draw list is sorted and remapped every frame) and a bundle per clip and image state, and it saves nothing on a frame where much changes (the scrolling list). Approach C is the only route to the 60 µs target. My reading of its costs: it is not worth a sub-project on this measurement alone; the UI pass (43.6 µs here) is now as large as the whole render side. 4h (layout and resolve retention for node trees, ceiling about 18 µs of the UI pass) is the smaller of the two and helps every changing frame, not only ones with few changes. The owner decides.

## 4h: the UI renderer (measured)

Spec: `docs/specs/2026-10-20-ui-renderer-design.md`. The UI draws through a renderer of its own (4g's approach C, reshaped by measurement): its commands become one 144-byte instance format in painter's order, culled before they are built, uploaded in one write and drawn through one pipeline that clips in the shader, one draw per texture run. The settings screen, every page of the `ui` demo and breakout's menus, HUD and play draw in **1 draw** (the old path: 64 on the settings screen); the demo and breakout retain tests now assert at most 2. Measured on an Apple GPU (Metal, M5 Pro), 1280x720, `benches/ui_render.rs`, on a shared machine (load 2 to 13 during these runs).

**The guard (Task 9: measured without the GPU's pacing).** `benches/ui_render.rs` measures the render step (`HeadlessEngine::render_time()`, wall time) of seven engines: an empty frame; the settings screen the targets were set on (`settings_tree_overflowing`, its list overflowing the panel), idle and with one label changing every frame, each with retention on and off; and the settings screen whose list scrolls (`settings_tree`), turned by the wheel every frame, with retention on and off. They are stepped in turn, one frame each, and before each step the guard waits until the GPU has finished all submitted work (outside the measure). Each median is over 2,000 rounds and is reported over the empty frame's median. Two things distorted the earlier figures:
- A lone engine is paced by the GPU. The GPU takes about 210 to 310 µs to finish a frame here (the guard's wait shows it), far longer than a 65 to 170 µs render step, so a step waits on the GPU's backlog. An empty frame waits most.
- Stepping the engines in turn does not cure this alone: seven frames of GPU work take longer than seven render steps, so the queue still backs up. Without the wait, four identical idle engines differ by 3 to 4 µs; with it, they agree to within 0.3 µs.

Wall time equals the render step's thread CPU time to within 0.5 µs this way (checked with a temporary probe), so the guard needs no platform call. It asserts the idle target, that one label is no slower retained than not, and that the scrolling list moves every frame (no frame kept). It prints one label against the 40 µs target without asserting it (missed today; Task 10 decides), and scrolling retained against retention off.

The guard as committed (Task 9's fix round; six runs of the 4h build, load 2 to 3):

| Frame | 4h | Target |
|---|---|---|
| empty frame's render step | 61.7 to 63.9 µs | |
| idle settings screen (kept) | +0.5 to +1.2 µs | under 5 µs: met (asserted) |
| **one label changing** | **42.7 to 46.0 µs** | under 40 µs: **missed** (printed) |
| idle, retention off | 78.4 to 89.5 µs | |
| one label, retention off | 80.1 to 91.2 µs | retained no slower: met (asserted) |
| scrolling, on the scrolling screen (no frame kept) | 52.0 to 55.5 µs | |
| scrolling, retention off | 44.0 to 47.5 µs | retained no slower: **missed** (printed; Task 11) |

Retention off moved between about +79 and +90 µs from run to run (two levels, within one session); the retained figures did not.

Against the pre-4h base (d1016380), the builds run alternately: first with every settings engine on the scrolling screen (Task 9's first round, before the targets went back to their own screen; five rounds, the base given the same fixture, load 3 to 4):

| Frame | 4h | pre-4h base | Target |
|---|---|---|---|
| empty frame's render step | 66.8 to 70.0 µs | 66.3 to 70.7 µs | |
| idle settings screen (kept) | -1.4 to +1.3 µs | -1.2 to +1.7 µs | under 5 µs: met |
| **one label changing** | **34.5 to 39.3 µs** | 64.3 to 73.8 µs | under 40 µs: met on this screen, narrowly |
| scrolling (no frame kept) | 51.6 to 58.3 µs | 81.5 to 91.7 µs | |
| idle, retention off | 43.0 to 48.0 µs | 70.2 to 78.1 µs | |
| one label, retention off | 44.2 to 50.1 µs | 72.0 to 80.5 µs | retained no slower: met |
| scrolling, retention off | 43.4 to 48.5 µs | 70.5 to 79.1 µs | retained no slower: **missed**, on both builds |

Single runs of the 4h build there read one label at 34.2 to 42.0 µs. Then on the screen the targets were set on (its list overflowing the panel, nothing scrolling), in two sessions:

| Frame (old screen) | 4h, load 2 to 4 | 4h, load 7 to 12 | base, load 2 to 4 | base, load 7 to 12 |
|---|---|---|---|---|
| empty frame's render step | 63.3 to 64.0 µs | 74.2 to 78.3 µs | 63.7 to 65.5 µs | 72.9 to 78.2 µs |
| idle (kept) | +0.6 to +1.0 µs | +1.1 to +2.2 µs | +1.0 to +1.4 µs | +1.1 to +2.4 µs |
| one label changing | 42.8 to 44.8 µs | 54.0 to 56.8 µs | 86.5, 88.0 µs | 102.9 to 105.5 µs |
| one label, retention off | 79.1 to 80.6 µs | 90.3 to 92.5 µs | 114.9, 117.1 µs | 127.4 to 132.6 µs |

So 4h roughly halves the changing frame's render side: one label costs 50 to 57% of the base's, round by round, in every session measured this way, and the profile below agrees (74 to 44 µs). Read on the screen it was set on, whose overflowing list draws 461 instances, the 40 µs target is missed (42.7 to 56.8 µs, by load). The scrolling screen draws fewer rows (355 instances), and there the same frame reads 34.2 to 42.0 µs. Task 10 decides the guard's assertions after it uploads only what changed.

Task 8's figures read otherwise because of the pacing. Its guard took each engine's 2,000 frames alone, so the empty baseline was paced (95 to 137 µs), and one label read 0 to 26.5 µs over it, with idle failing once (7.2 µs). Its interleaved probe (five engines in turn, thread CPU, no wait) was paced the other way: one label 58.4 to 62.5 µs (base 106.3 to 107.2), idle 5.0 to 7.5 µs (base 3.8 to 5.6).

**The idle frame (Task 9).** Measured without the pacing, an idle (kept) frame costs the same on both builds: +0.6 to +2.2 µs for 4h and +1.0 to +2.4 µs for the base on the old screen, and within 1.5 µs of the empty frame on the scrolling screen. Stepped in turn without the wait, both builds' kept frames read +7.3 to +10 µs, so the rise Task 8 saw came from the GPU's backlog, not from work in the kept frame. Profiles agree (xctrace Time Profiler; 100,000 frames each of each build's idle and empty screens, two pairs per build, with the wait; main thread under `Engine::draw_frame`; traces deleted):
- On both builds, the kept frame's only work beyond an empty frame is `UiRetain::keep` comparing the commands: 0.9 to 1.2 µs.
- Everything else (buffer creation, submits) moves by 1 to 2 µs between pairs, in both directions. The base's idle read +9.2 µs over its empty frame in one pair and -0.3 µs in the other.
- One call shows only in 4h's idle traces: Metal waking its completion queue on commit (`_dispatch_kq_poll` under `commitCommandBuffer`, 1.2 to 1.6 µs). It is the same submit every frame makes, timed by the system, not a call 4h added.

Nothing to fix.

**Profile of the changing frame** (Task 8, on the old screen: `settings_label`, retained; xctrace Time Profiler, 40,000 frames, main thread, inclusive µs per frame, against an empty scene run the same way; traces deleted):

| Function | Label | Empty | Difference |
|---|---|---|---|
| `Engine::frame` | 199.3 | 101.5 | +97.9 |
| `SceneManager::ui_pass` (the UI pass, not render side) | 50.9 | 0.9 | +50.0 |
| `Engine::draw_frame` (the render side) | 137.3 | 93.3 | **+44.0** |
| `UiRenderer::finish` | 19.8 | 0 | +19.8 |
| of which `UiGpu::upload` (one `write_buffer`, 461 instances, 66,384 B) | 19.4 | 0 | +19.4 |
| `build_ui` (of which the text cache 6.1) | 13.5 | 0 | +13.5 |
| `UiRetain::keep` | 2.9 | 0 | +2.9 |
| `WgpuRenderer::render_frame` (encoding and submitting; most of the difference is the `ui` pass's render-pass begin) | 81.1 | 76.0 | +5.1 |
| `CameraUniforms::sync` | 12.7 | 11.3 | +1.4 |
| `Queue::write_buffer` (all calls) | 35.1 | 15.2 | +19.9 |

Each scene ran alone, so both runs are paced (the empty run's render side is 93.3 µs, against 63 to 78 µs for an empty frame without pacing). Their difference, +44, still agrees with the guard's figures for this screen (+42.8 to 56.8 µs, by load). The research's profile of 4g's frame read +74 µs by this method (`emit_ui` 17.9, `Batch2D::prepare` 25.6, encoding the UI pass 23.2, keep, sort and submit 7.2): the encode is now +5 (one draw), `prepare` +0 for the UI, and `build_ui` (13.5) does what `emit_ui` and `prepare`'s placing did.

**Criterion** (`cargo bench --bench ui_render`, whole `step()`): Task 8's two runs (load about 13, then about 4.5), and Task 9's in one session (load 2 to 4). Idle and label are on the screen the targets were set on, except in the "all on the scrolling screen" column; the list scrolls in the last two Task 9 columns only:

| Bench | Task 8 | Task 9, before the fixture change | Task 9, as committed | Task 9, all on the scrolling screen | 4g |
|---|---|---|---|---|---|
| `settings_idle` | 121.5, 155.2 µs | 123.6 µs | 121.7 µs | 120.5 µs | 122.7 µs |
| `settings_idle_unretained` | 213.0, 225.6 µs | 216.2 µs | 215.1 µs | 176.2 µs | 254.8 µs |
| `settings_label` | 186.0, 190.2 µs | 180.1 µs | 179.6 µs | 172.2 µs | 224.8 µs |
| `settings_label_unretained` | 229.4, 231.2 µs | 223.3 µs | 222.7 µs | 185.5 µs | 259.7 µs |
| `settings_scrolling` | 123.3, 126.2 µs (kept frames) | 123.7 µs (kept frames) | 179.4 µs | 182.6 µs | 216.1 µs |
| `settings_scrolling_unretained` | 227.7, 219.2 µs | 218.0 µs | 174.5 µs | 179.7 µs | 249.9 µs |
| `empty_frame` | 189.7, 199.1 µs | 187.4 µs | 187.3 µs | 189.8 µs | 187.1 µs |

On the targets' screen a changing frame's whole step is 35 to 45 µs faster than 4g's retained, and 28 to 36 µs faster not retained. The idle and empty rows are paced by the GPU (an idle retained screen steps faster than an empty frame) and move with the load; the guard, not these, measures the render side.

**A settings screen that scrolls (Task 9).** The shared fixture's scroll area grew to its content (1,406 units in a 460-unit panel), so the wheel had nothing to move and `settings_scrolling` measured kept frames (at 4g's commit it redrew, but with identical commands, so it never measured a moving list either). `settings_tree` now gives it the rest of the panel (`fill_height` in place of `grow(1.0)`: 407 units of view, 999 of travel), so the wheel scrolls it; `settings_tree_overflowing` keeps the old layout for the guard's targets. The closure mirror of the screen in `benches/ui.rs` (and its copy in `tests/ui_alloc.rs`) changed with it, so 4f's node-against-closure comparison stays like for like:
- `settings_scrolling` asserts that the list's widgets move, and the guard that no scrolling frame is kept. The wheel turns 20 lines each way, less than the list's travel, so the list never rests at an end.
- Scrolling, a retained frame is **slower than one without retention**: by 8 to 11 µs on 4h, and 10 to 13 µs on the base. While the list moves, 41 of the 44 visible texts miss the text cache every frame: it is keyed by each text's origin, so they are built and stored again, and only the tabs' 3 are reused. Without retention they are only built. Reusing a moved text's instances by translation (4g's noted follow-up) is the fix: Task 11.

What the change moved (Task 9, same sessions, before and after):
- The guard: nothing, as it reads idle and one label on `settings_tree_overflowing`. On the scrolling screen the same frames read lower, at the same load (one label +34.5 to 39.3 µs against +42.8 to 44.8 µs; with retention off +44.2 to 50.1 µs against +79.1 to 80.6 µs): the list ends at the panel (407 units of rows shown) where it runs on to the bottom of the screen (545 units), and culling drops the rows outside it, so the label frame uploads 355 instances (51 KB), not 461 (66 KB).
- Criterion: as in the table above.
- `benches/ui.rs` (before, then after): the node screen with the focus moving 34.1, then 32.6 and 32.7 µs (the focus now scrolls the list); unchanged 33.0, then 32.5 µs, still 3 built and 274 replayed; the dirty check 1.28, then 1.29 µs; spawning 128.0, then 130.8 µs. Updating the unchanged tree read 37.6, then 41.5 and 38.6 µs: noise, as the closure screen, which did not change, moved 2% in the same runs.
- `benches/ui.rs`, the closure screen brought into step: 49.1, then 47.3 µs; the node screen, whose code did not change, read 32.4, then 31.7 µs in the same runs, so the mirror's own change is about 2%. The node screen is now 33% under the closure one (4f: 30%).
- `tests/ui_cache_alloc.rs`: unchanged, 14 allocations cached and 189 uncached (ceiling 30). `tests/ui_alloc.rs` (the closure screen): unchanged, 186 allocations (ceiling 220).
- The tests that use the fixture pass unchanged: the node cache oracle's four settings scenarios (the pad walk now scrolls the list), the rebuilds overlay's retain test, and the e2e renderer and retain tests (whose wheel step now scrolls).

**`benches/text.rs`** (640x360; the 4h build and the pre-4h base run alternately, two rounds each): HUD of 100 texts and 50 panels 194.0, 195.7 µs (base 204.9, 190.9; 4g 197.2), and with `ui_retain(false)` 225.1, 216.3 µs (base 230.6, 238.0; 4g 227.2); 1,000 cached UI texts 137.2, 135.2 µs (base 135.7, 134.0; 4g 136.1); 200 world labels 153.8, 228.6 µs (base 188.4, 225.0; 4g 146.6). The HUD and the cached texts are kept frames (unchanged UIs), so 4h leaves them as they were; the HUD without retention, which builds and draws every frame, is 5 to 22 µs faster. The world labels bench swings between about 150 and 230 µs on both builds alike, so the world's text is unchanged within that noise.

**Uploading only what changed: now worth doing, in its simplest form.** The upload is now the largest part of a changing frame: 19.4 µs, under half of it (of +44 profiled on the old screen, where the guard read +42.8 to 56.8 µs). Inside it, 11.3 µs is the copy into a new staging buffer and 6.7 µs creating that buffer (`wgpu` makes one per `write_buffer` call). The research's numbers say one short write costs about 2.2 µs against 9 for 51 KB, so writing only the changed bytes could save about 10 to 12 µs, taking the profiled frame to about 32 µs. On the scrolling screen the frame uploads 355 instances (51 KB), so the saving is a little smaller; the guard (Task 9) is the instrument to measure it. The settings screen is the good case: the changing label is recorded last, so its instances sit at the end of the array and a compare with the last frame finds one short dirty range. It pays only while instance positions hold: a text whose glyph count changes (99 to 100) shifts every later instance, and a scrolling list moves them all, so the diff needs a fallback to one full write beyond a few ranges (the research put the break-even at about 5) and the compare itself (about 1.5 µs for 66 KB, estimated). After the upload, the next parts are `build_ui`'s text copies (6.1 µs) and the `ui` pass's own begin and encode (about 5 µs, near the floor of one more pass).

**Next: analytic line anti-aliasing (owner, 2026-10-20).** UI line pixels may differ from the pre-4h renderer on Apple GPUs by one derivative class: Metal gives the old indexed, shared-vertex line mesh a different `fwidth` near a shared edge than the UI renderer's per-triangle drawing (the new path equals the old line pipeline drawn without shared vertices; on the demo tour only the text demo's page 5 differs, 34 pixels at miter tips, at most 67/255). Accepted. The follow-up is a small sub-project of its own: derivative-free, analytic anti-aliasing for UI and world lines, so the coverage of a line pixel is computed from its distance to the line's edges in pixels rather than from `fwidth`, and lines draw the same on every GPU and in every mesh layout. It starts with a visual review of how others do it: Skia (analytic coverage for strokes and paths), Dear ImGui and NanoVG (a one-pixel feather fringe of extra geometry, alpha 0 at its outer edge), and Unity UI Toolkit (its vector stroke tessellation and edge anti-aliasing), then compares the candidates on the demo pages by eye.

## Left for Later

From 4b's out-of-scope list:
- touch input;
- analog action axes and more than one gamepad;
- screen readers (AccessKit);
- right-to-left layout;
- rich text editing in a text field;
- windows the player drags or resizes.

From 4c: node versions of `ui.custom` painters and absolute `draw_*` calls, which stay closure UI.

Found in 4f, in the text engine: text that embeds icons (`[icon=..]`, and `[prompt=..]` markup resolved to a glyph) keeps its old layout after a sprite is replaced in place at another size (a hot reload), with node caching on or off. The layout is keyed by the text, not by the sprite's version; an image or a `ui.prompt` glyph sized by a sprite already rebuilds (`Read::Sprite`).
