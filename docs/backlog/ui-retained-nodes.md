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

The research measured the render side of this screen at about 120 µs (profile, `emit_ui` 70.6 + `Batch2D::prepare` 28.0 + about 19 for the extra pass); idle is now zero and a changing frame is about 80 to 95 µs, but the 60 µs target for a changing frame is missed. The 60 µs one-label target was **not met** (81 to 96 µs). Approach B's own share of that frame is about 21 µs (`emit_ui` 18.4 + `keep` 2.4); the rest is the shared batch and encode path that the spec left to approach C. The guard asserts the idle and retained-no-slower targets and reports the one-label target.

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

**Approach C** (upload only the changed instance ranges; record the UI's draws once as a render bundle). Render-side work over an empty frame in a changing frame is about 69 µs: `emit_ui` 18, `Batch2D::prepare` 27 (its uploads about 14 more than empty), and the extra pass about 24 (`encode_render_pass` +20). So prepare, uploads and encoding are about three quarters of what is left, and the per-call costs (one staging buffer per `write_buffer`, about 150 draws encoded) dominate their size. A bundle of the unchanged draws plus a small pass for the changed ones could remove most of the encoding and upload, perhaps 25 to 35 µs, which would bring this frame under 60 µs, but it needs stable instance ranges across frames (the draw list is sorted and remapped every frame) and a bundle per clip and image state, and it saves nothing on a frame where much changes (the scrolling list). The remaining render-side cost of a changing frame is the batch's prepare and upload (about 27 µs) and encoding the UI pass (about 20 to 24 µs); approach C is the only route to the 60 µs target. My reading of its costs: it is not worth a sub-project on this measurement alone; the UI pass (43.6 µs here) is now as large as the whole render side. 4h (layout and resolve retention for node trees, ceiling about 18 µs of the UI pass) is the smaller of the two and helps every changing frame, not only ones with few changes. The owner decides.

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
