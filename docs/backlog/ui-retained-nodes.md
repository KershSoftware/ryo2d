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

## 4e: Fast Big UIs

- Subtree caching: skip the walk and the builders for subtrees whose components did not change. For 4c's walk, each node costs:
  - a few ECS component reads (about 8 to 10 ns each, some 560 a frame on the settings screen);
  - one or two ids;
  - the builder's work.
- Virtualised lists for node and closure UI: record only the rows in view. The 1,000-row scroll list bench shows the need.
- Baseline: the node settings screen at 112 µs, closure 104 µs (above).

## Left for Later

From 4b's out-of-scope list:
- touch input;
- analog action axes and more than one gamepad;
- screen readers (AccessKit);
- right-to-left layout;
- rich text editing in a text field;
- windows the player drags or resizes.

From 4c: node versions of `ui.custom` painters and absolute `draw_*` calls, which stay closure UI.
