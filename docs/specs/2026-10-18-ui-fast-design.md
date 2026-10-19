# Fast Big UIs (4e) Design

## Overview

Sub-project 4e of the professional-grade overhaul makes the UI cheap to run and big lists cheap to show. It sits on 4b (interaction), 4c (retained nodes) and 4d (UI files): `docs/specs/2026-10-16-ui-interaction-design.md`, `2026-10-17-ui-nodes-design.md`, `2026-10-17-ui-files-design.md`.

What it adds:

- **A cheap frame.** The measured per-frame costs of the shared builder pipeline are cut: SipHash on UI and text keys, text hashed twice per leaf, buffers rebuilt every frame, a full layout diff, theme looks cloned per widget, and the node walk's repeated component reads. Closure UI and node UI both get faster; behaviour does not change.
- **Virtual lists.** Fixed-size rows (and grid cells): a closure `ui.list(..)` that builds only the rows in view, and a pooled node `List` whose few row entities are reused and bound to items by a `UiListBind` message.
- **A new guard** for node UI against closure UI, and absolute frame budgets.
- **A measurement at the end:** with 4e landed, the frame is profiled again and a recommendation on node subtree caching is written for the owner.

**Why:** the backlog note `docs/backlog/ui-retained-nodes.md` gave 4e subtree caching and virtualised lists. A profile of the UI benches (xctrace Time Profiler, before 4e) showed the frame's time is mostly not in what caching would skip:

| Bucket | Closure settings (98 µs) | Node settings (108 µs) | 1,000-row list (579 µs) |
|---|---|---|---|
| SipHash hashing | 39 µs (40%) | 40 µs (37%) | 258 µs (45%) |
| Allocation | 11.5 µs | 13 µs | 43 µs |
| Layout engine | 12 µs | 13 µs | 51 µs |
| Node walk (own cost) | none | 9 µs | none |
| Ids, memory, navigation, theme, tree resolve | about 2% or less each | about 2% or less each | about 2% or less each |

The largest single cost is each text leaf hashed twice per frame with SipHash (its seed in the builder, then its key in the text engine), 25-35% of a frame. In quiet runs the node settings screen measured 10.4% over the closure one, past 4c's 10% guard.

**Decisions taken with the owner:**

- **Scope:** the cheap frame and virtual lists; then measure again and decide whether node subtree caching is worth it.
- **Guard:** the node settings frame may cost at most the larger of 10% or 10 µs more than the closure one, plus absolute budgets. A pure ratio would fail once the closure frame gets much faster, though both are faster.
- **Row sizes:** fixed (one row height per list, one cell size per grid). Variable, measured rows are out of scope.
- **Node lists:** pooled and bound by message (the model/view design of Unreal's ListView, Unity's ListView, Android's RecyclerView): a screenful of reused row entities, not one entity per item.
- **Acceptance:** benches, plus one demo page with a 10,000-row list in both styles.
- **Approach A** for both: targeted fixes in the existing pipeline (not a rewrite of layout), and lists built inside the scroll area with spacers, shared by closure and node UI.

## Part 1: The cheap frame

Each change targets a measured cost. None changes what a frame draws or how it behaves.

### One fast hasher

- 4c's folded-multiply id hasher (`IdHasher`, `src/core/ui/id.rs`) becomes a shared engine module, so the UI and the text engine use the same hasher. It hashes words, not bytes, and needs no new dependency.
- It replaces SipHash (`std`'s default `RandomState` and `DefaultHasher`) for:
  - every `UiId`-keyed map: `UiMemory`'s records (`prev`, `next`), states and focus memory; the layout engine's entries and its per-call index; navigation's maps;
  - the text engine's caches (`cache`, `lines`) and the keys they are looked up with.
- These keys are made by the engine from its own data, so SipHash's resistance to crafted collisions buys nothing here.
- This settles the question 4c left open (the memory and layout maps): both styles get faster, and the new guard (below) keeps the node-against-closure comparison meaningful.

### Text hashed once

- A text leaf's seed (its text, style and layout options) is computed once per frame with the fast hasher.
- The seed is the text engine's cache key for both measuring and laying out, so the text is not hashed again inside the engine. The key still includes the scale and the font generation.
- Equal text, style and options give an equal key whether they come from a closure label, a node's `UiLabel`, or a binding pattern.
- A cache hit is confirmed by comparing the stored text, style and options with the request before it is used, so a hash collision can never draw the wrong text; the comparison costs far less than the SipHash it replaces.

### Buffers kept between frames

- The subtree's node and draw lists, the layout engine's descriptions, results and index map, and the per-frame scratch vectors of tree resolve are kept in their owners and cleared at the start of each use, not allocated anew.
- Label text is not copied into a new string per frame where a borrowed or shared one serves. As built, a node's `UiLabel` is still copied into its text each frame (`nodes/draw.rs`, `rich`): `UiLabel(String)` is public, and the targets were met without changing it. Drawing it without the copy is in the backlog.

### A cheaper layout diff

- Each layout node carries a hash of its style, parent flow and measure. The diff compares that hash with last frame's. Only a changed node is cleaned up (the style clone), and only a change is passed to the layout library.
- An unchanged subtree therefore costs a walk and a comparison per node, and the layout library recomputes nothing.

### Theme looks borrowed

- A widget reads its look from the theme by reference instead of cloning it every frame.

### A cheaper node walk

- Each node's components are read in one lookup per node, not one per component.
- The reload check (`reload_trees`, 4d) keeps its query between frames, so it no longer builds one every frame once file trees exist.
- 4d's "built before a reload, spawned after it, never reloads" defect is fixed: a tree's source records the file generation it was built from, not the one current at spawn.

### Targets

Measured in quiet runs (no other heavy process) on the owner's machine, recorded in the backlog note:

| Bench | Before | Target |
|---|---|---|
| Closure settings screen | 98 µs | under 60 µs |
| Node settings screen | 108 µs | within the guard of the closure one |
| 1,000-row plain scroll list | 579 µs | under 350 µs |
| Empty frame | 180 ns | about the same |

Nothing changes behaviour: every existing test passes, and `./dev demos:tour` captures every page pixel-identical to before 4e.

### The guard

The node settings frame may cost at most `max(10%, 10 µs)` more than the closure settings frame. It replaces 4c's 10% ratio in `benches/ui.rs`'s notes, `src/core/ui/AGENTS.md` and the backlog note.

## Part 2: Virtual lists

Fixed-size rows only: one row height per list, and with `columns(n)`, cells of one size laid out in rows.

### Shared shape

- A list is a scroll area whose content length is exact: `rows × (row_height + gap) - gap`, with `rows = ceil(count / columns)`. The scroll area takes this length whatever the rows hold, so a row whose contents overflow its height never changes it.
- Each frame it builds only the rows in view plus `overscan` rows on each side (default 2), placed between a leading and a trailing spacer, so the normal layout places them and the scroll area clips and translates them as it does any child. Building, laying out, recording and drawing cost what the built rows cost, whatever the count.
- **Ids follow the index.** Each row is built inside `ui.scope(index, ..)`, so a row's id, its hover and press, its focus and its widget memory belong to the item, not to a screen position.
- **The focused row stays built.** A row with the focus (or about to get it from a jump) is built even when it is outside the view and the overscan (scrolled away by the wheel or the scrollbar), so 4b's rule (a focused widget not drawn loses the focus) never fires for it. It is clipped like any row out of view.
- **The pad walks the whole list.** The overscan means the row past the edge of the view is always built, so a direction moves the focus onto it and the scroll area's existing scroll-into-view brings it in. In a grid, left and right move within a row and up and down by `columns`, by the normal spatial navigation.
- **Jumping to an index.** A list can be scrolled to any index (the offset is computed from the index, so the row need not have been built). A jump further than one view first moves the offset to one view short of the index and eases the rest, so it never sweeps through the rows between. The row at that index can be given the focus, which applies when it is built and drawn (in closure UI the first focusable widget built in the row, in node UI the first focusable node), up to two frames later.
- **Per-row widget memory** (a text field's caret, an open dropdown) is dropped when the row leaves the built range, like any widget not drawn. State that must outlive scrolling belongs in the game's data.
- **Overscan** is clamped to at least 1 (so the pad can always step past the edge).

### Closure UI

```rust
let list = ui.list(scores.len())
  .row_height(28.0)
  .gap(2.0)
  .fill()
  .show(|ui, i| {
    ui.row().gap(8.0).show(|ui| {
      ui.label(format!("{}", i + 1));
      ui.label(&scores[i].name);
    });
  });
// list.range: the indices built this frame; list.id: the list's id
```

- `ui.list(count) -> List`: a builder taking the scroll area's size and placement options (`width`, `height`, `fill`, `max_height`, ...), plus `.row_height(h)` (required; a list without one warns once and draws nothing), `.columns(n)`, `.gap(g)`, `.overscan(rows)` and `.id(key)`. Lists scroll vertically.
- `.show(|ui, index| ..)` returns a `ListResponse` with the list's `id`, the built `range`, and the scroll area's response.
- `ui.list_scroll_to(list_id, index)` scrolls so the row at `index` is in view (the least movement, as `scroll_to` does); `ui.list_focus(list_id, index)` also gives it the focus.

### Node trees: the pooled `List`

```rust
// In code
let board = ctx.spawn_ui(
  UiNode::list(10_000, 28.0)
    .gap(2.0)
    .fill()
    .row_shape(|| UiNode::row().gap(8.0).child(UiNode::text("{rank}").bound()).child(UiNode::text("{name}").bound()))
    .name("board"),
);
```

```ron
// In a file
List(count: 0, row_height: 28, gap: 2, fill: true, name: "board", row_shape: LeaderRow)
```

- `UiNode::list(count, row_height)` with `.columns(n)`, `.gap(g)`, `.overscan(rows)`, the scroll area's size options, and `.row_shape(f)`: a function returning one row's shape (`Fn() -> UiNode`). In a file, `row_shape` names a template (built with its defaults) or is a node value. A list without a row shape warns once and builds no rows.
- **The pool.** The list keeps one row entity per built index (the rows in view, the overscan and a focused row): about a screenful, whatever the count. Each row carries `UiListRow { index }`. When the view moves, rows whose index left the built range are reused for indices entering it; extra rows are despawned when fewer are needed; new rows are spawned from the row shape when more are needed.
- **Binding.** Every row given an index (new, or reused for another index, or refreshed) sends `UiListBind { list, row, index }` as a message (and as an entity event on the row, for observers). The game fills the row:
  - with `ctx.ui_set(row, name, value)`: **each row is its own binding scope**, so a row's `{name}` patterns and bound widgets read that row's values, not the list's root;
  - or by writing the row's nodes' components (found with `ctx.find_ui_in(row, ..)`).
- **When binding happens.** A scene's `update` runs at the fixed rate (not every frame) and after the UI pass, and its `ui` runs every frame after its node trees are drawn. So the pool updates once a frame at the start of the UI pass, after this frame's wheel, scroll requests and navigation are resolved and before any node tree is drawn, covering the view at the scroll area's current offset and at its target (and the rows between when the two are within two views), plus the overscan, a focused row and a row being jumped to. A row given an index in a frame is drawn from the next frame, so the game binds it in between: in `ui` (recommended: it runs every frame), in an observer on the row (immediately), or in `update`. A new list's first frame shows no rows; before it has been drawn once, its view is unknown and the pool fills the screen's height. A row needed only because the view moved further than the overscan within one frame (dragging the scrollbar far) is added the next frame; until then that space is empty.
- **Ids are keyed by index**, as in closure UI: a row is drawn inside `ui.scope(index, ..)`, so a reused row entity never carries another item's focus, hover or widget memory.
- **Game API** (on `SceneContext`, and on `UiHarness` for tests):
  - `ctx.ui_list_count(list, n)`: change the item count (rows past the end are released; the view is clamped). The count is state, like `UiValue`: a reload or `update_ui` writes it only when the tree's count changed;
  - `ctx.ui_list_refresh(list)`: send `UiListBind` again for every built row (after the game's data changed); `ctx.ui_list_refresh_row(list, index)` for one;
  - `ctx.ui_list_scroll_to(list, index)` and `ctx.ui_list_focus(list, index)`;
  - `ctx.ui_list_index(node) -> Option<usize>`: the index of the row a node belongs to, so a `UiEvent` from a widget in a row maps to the item.
- **Events** from widgets in rows are the ordinary `UiEvent`s (4c): their `node` is the widget's entity, and `ui_list_index` gives the item.
- **Reconcile and reload.** The list's own settings (count, row height, columns, gap, row shape) are tree-owned (4d's reconcile rules). A changed row shape (a reload of the row template, or `update_ui` with a new `.row_shape`) rebuilds the pooled rows from it and binds them all again. The pool's rows are the list's own; they are never matched against a tree's children.
- **Spawning and despawning** rows uses 4c's spawn and despawn; a despawned list despawns its rows.

### Files

- `List` joins the vocabulary: constructor options `count` (a whole number, default 0), **`row_height`** and **`row_shape`** (a template name or a node), both required; options `columns`, `gap`, `overscan`, the scroll area's size and placement options, and the universal options.
- `ryo2d check-ui` checks `List` like any kind.

## Acceptance

### Benches (`benches/ui.rs`)

- The cheap-frame targets above.
- A 10,000-row closure virtual list scrolled to its middle: **under 50 µs** a frame.
- The same list as a node `List`: within the guard (`max(10%, 10 µs)`) of the closure list.
- Count independence: the 10,000-row closure list costs within 10% of a 100-row one scrolled the same way.

### The "Big list" demo page

- A new page in the `ui` demo: a 10,000-entry leaderboard (rank, name, score) as a closure list beside the same leaderboard as a file-built node `List` filled from `UiListBind`, with a "jump to rank" field.
- Pad-walkable end to end in both lists. Acceptance tests drive the pad from the top to the bottom of each list, jump to an index, and check the focused rank and the rows shown.

### Tests

- **Cheap frame:** every existing test passes; the demo tour's captures are pixel-identical; unit tests for the hasher's distribution on ids and text keys; the layout diff skips unchanged nodes (the layout library is not told about them); buffers are reused (no growth across frames of the same screen).
- **Lists, shared:** the built range and spacers for top, middle and end; columns; gap; overscan clamping; content length; a list shorter than its view; count 0.
- **Focus:** walking off the edge in both directions, to the first and last items; the focused row staying built when scrolled away and focus resuming from it; jumping to an index with and without focus; ids following the index (focus stays on the item when the list scrolls).
- **Pool:** rows reused across a scroll; `UiListBind` sent for each new or reused row, before the row is drawn; count changes (grow, shrink, to zero); refresh of all and of one row; `ui_list_index` from a nested widget; per-row bindings; reload of the row template rebinding all rows; a despawned list despawning its rows.
- **Files:** `List` in the vocabulary and in `check-ui`.

## Documentation

- The `ryo-ui` skill: a virtual lists section (closure `ui.list`, node `List`, binding, the game API, focus rules, what is and is not kept when a row leaves the built range) with recipes, a leaderboard and an inventory grid. `ryo-api` and `ryo-messages` (`UiListBind`). Each skill copied byte-identical to `src/cli/skills/`.
- `src/core/ui/AGENTS.md`: the hasher, the layout diff, lists and the pool, the new guard.
- `docs/backlog/ui-retained-nodes.md`: 4e done, the before and after numbers, and the subtree caching recommendation.

## The end of 4e: measure again

With all of 4e landed, profile again (the same method: xctrace Time Profiler on the settings, node settings and list benches) and write a short recommendation into the backlog note: whether node subtree caching (skipping the walk, the builders, the layout and command emission for subtrees whose components and interaction did not change) is worth its machinery, with the numbers behind it. The owner decides then.

## Design decisions

- **Fix what was measured.** The profile, not the backlog's guess, sets the work: hashing, allocation and the layout diff are most of a frame; ids, memory, navigation and theme lookups are not worth targeting.
- **A word-wise hasher for engine keys.** Every key is the engine's own data.
- **The guard measures the node walk's overhead**, not a ratio that punishes the shared path for getting faster.
- **Lists inside the scroll area.** Row placement reuses the layout and the scroll area's clipping, translation, wheel, scrollbar and scroll-into-view, so lists add little new machinery and both styles share it.
- **Ids keyed by index.** State follows the item, so the pool's reuse of entities is invisible.
- **Binding by message.** No closures over the game's data in the engine; the game fills rows where it already handles events, and observers can bind on the spot.

## Risks

- **Pixel-identical output.** Hashing and buffer changes must not change a draw; the demo tour comparison guards it.
- **Hash quality.** The fast hasher's distribution is tested on ids and on similar text keys; text cache hits are confirmed by comparison, so a collision costs a miss, never wrong text. `UiId`-keyed maps rely on ids being well mixed already (4b's id mixing).
- **One-frame gaps in node lists** when the view jumps beyond the overscan in a frame (documented; overscan can be raised).
- **Per-row state loss** when rows leave the built range (documented).

## Out of scope

- Variable, measured row heights.
- Node subtree caching (pending the end-of-4e recommendation).
- Virtualising the dropdown and choice lists (they stay short).
- Horizontal virtual lists.
- Touch flinging and momentum.
