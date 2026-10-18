# Retained UI Nodes (Sub-projects 4c, 4d, 4e)

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
- Node subtree caching (pending the owner's decision above).
- The dropdown and choice lists virtualised (they stay short).
- Touch flinging and momentum.

## Left for Later

From 4b's out-of-scope list:
- touch input;
- analog action axes and more than one gamepad;
- screen readers (AccessKit);
- right-to-left layout;
- rich text editing in a text field;
- windows the player drags or resizes.

From 4c: node versions of `ui.custom` painters and absolute `draw_*` calls, which stay closure UI.
