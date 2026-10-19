# Cached Node Trees (4f) Design

## Overview

Sub-project 4f of the professional-grade overhaul makes retained node trees incremental: a frame rebuilds only the node subtrees whose inputs changed, and replays the rest. It sits on 4c (retained nodes), 4d (UI files) and 4e (the cheap frame and virtual lists): `docs/specs/2026-10-17-ui-nodes-design.md`, `2026-10-17-ui-files-design.md`, `2026-10-18-ui-fast-design.md`.

What it adds:

- **Build records.** Each node remembers what its builder produced the last time it ran, in a form that can be replayed anywhere.
- **Splice or build.** The node walk replays a clean subtree's records instead of calling its builders, and builds a dirty one as today.
- **Dirty tracking** from change ticks, interaction roles, live kinds, reads of other nodes and the environment.
- **Tooling:** a `debug:ui-rebuilds` overlay, a switch that turns caching off, harness counters and a debug-build spot check.
- **A measurement at the end:** whether layout and paint retention is worth a later sub-project.

**Why:** retained UI in serious engines is incremental (Unity UI Toolkit, Godot's canvas items, Flutter's element tree): work follows change. 4c's node walk drives 4b's builders for every node every frame. After 4e, the node settings screen costs 60.2 µs, of which the walk is 8.5 µs and the builders 28.1 µs (`docs/backlog/ui-retained-nodes.md`, 4e). 4e recommended against caching for speed alone; the owner chose it as an architectural property of the engine.

**Decisions taken with the owner:**

- **Purpose:** professional parity: node trees are incremental by design, not only for today's numbers.
- **Success bar:** deterministic rebuild counts, plus a bench target for an unchanged node frame, plus a pixel-identical demo tour.
- **Exposure:** automatic for every node tree, with a debug view of what rebuilt and a switch to turn caching off.
- **Approach A:** incremental build (splice clean subtrees). Layout, resolve, records and draw commands run as today. Layout and paint retention (approach B) is left for a later sub-project, measured against this one.

## What a frame does today

A builder call, from a node or from closure code, pushes tree nodes and draws into the open subtree (`tree.rs` `Subtree`), makes ids (`IdScopes`), touches widget state (`UiMemory::state`, which stamps it as seen) and may queue a popup or a tooltip. Everything a builder pushes is relative: draws are insets and fractions of their node, and a node's parent, depth, focus scope and enclosing scroll area come from the open context. Absolute rects, snapping, clips and scroll shifts appear only in `tree::resolve`, which lays the subtree out (4e's hash diff keeps unchanged nodes cheap), writes interaction records into `UiMemory` and turns draws into `UiCommand`s.

So a subtree's builder output is position-independent: replayed into a different place in the stream, it resolves correctly.

## Build records

Every node entity in a drawn tree has a **build record**:

- its own output in push order: its tree node or nodes and its draws, with a slot where each child's output goes;
- the ids it made, the widget-state ids it touched, whether it is the walk's Back target (a modal or popup container), and the other nodes' ids it read (`nav_*` targets, a popup's anchor);
- the change tick and the environment generation it was built at.

Records are **relative**. A node's parent is "the node open where I am placed" or an index within the record; depth, layer, clip, the enclosing scroll node and the focus scope are inherited from the open context unless the node opens its own (a focus scope, a scroll area, a clip). Draw order is a sequence, not numbers.

Records live in the existing `UiNodes` resource, keyed by entity. No component is inserted on the game's entities and no resource is added (4e's rule: a UI pass adds nothing to a world the game did not ask for). A record is dropped when its entity is despawned or is not drawn in a frame, as widget state is.

## Splice or build

For each node, the walk asks whether its subtree is clean:

- **Clean:** the walk replays the subtree's records depth-first into the open subtree. No component is read and no builder is called.
- **Dirty:** the walk builds the node as today. The builder runs, its output is recorded into the node's new record, and the walk recurses into the children, where each child again replays or builds.

A dirty node makes all its ancestors dirty, never their siblings. So a replay never resumes inside a half-open container: an ancestor on a dirty path re-runs its own builder call (cheap) and replays its clean children. A change costs its path from the root plus the changed subtree.

This covers root trees, embedded trees (`ui.node(root, world)` replays into the closure's open container), modal and popup containers, and node-list rows (a clean row replays; the `List` node itself always builds, 4e).

## Replay fidelity

A replay pushes exactly what a rebuild would:

- **Draw order:** each replayed node and draw takes `next_order()` in its recorded sequence, so the counter reaches the same numbers. Popups renumber later as today.
- **Context:** parent, depth, layer, clip, the enclosing scroll node and the focus scope are stamped from the open context at the splice point, as `push_node` does.
- **Ids:** the recorded ids are registered in the frame's used set, and in the capture when a list's focus watch is collecting a row's ids. Node ids are keyed by entity or index, so they equal a rebuild's.
- **Widget state:** each recorded state id is stamped as seen, so `end_frame` keeps scroll state, dropdown memory, timers and list state.
- **Text:** text leaves replay with their text keys, so resolve still gets text-cache hits.
- **Popups and modals** in node trees replay into the popup queue with their anchor; the walk's Back target is restored from the record.
- **No allocation once warm.** Text leaves and heavy draw payloads are shared (`Arc`), and records keep their buffers. Only a rebuilt node re-records.
- **Never replayed:** tooltips and drag ghosts (hover and drag make their nodes dirty); events, component writes, sounds, focus requests, rebinds and input consumption, which only a built node produces.

## What makes a node dirty

A node is dirty when anything its builder reads may have changed since its record was made:

1. **Its components.** The UI's tracked components: `UiNodeKind`, `UiRoot`, every state component (`UiLabel`, `UiValue`, `UiChecked`, `UiIndex`, `UiOptions`, `UiTextValue`, `UiSlot`, `UiImage`, `UiTooltip`, `TextReveal`), the list row's index (`UiListRow`), `Children` (adds, removes, reorders) and `ChildOf` (reparenting); and, through removal readers, a removed `UiHidden`, `UiDisabled`, `UiTooltip` or `UiBind`. The set is the union of what `NodeRead` and the per-kind draws read; the plan lists it from the code. Bindings and the reconcile write these components only on a real change (4d), so neither needs its own signal. The walk's own writes (a widget's value written back, `UiInteraction`) come from nodes that are built that frame and are stamped into their new record, so they do not dirty it again.
2. **Interaction.** A node whose id holds an interaction role this frame or last: hot, hovered, pressed (active), focused, drag source, drop hover, or a one-frame result (clicked, double click, activated, gained, lost, dropped, drag started or released). It draws its new state, and its exit from the old one. A focused widget rebuilds every frame it holds the focus: it reads keys, and a text field blinks its caret.
3. **Live kinds,** rebuilt every frame while live: a scroll area whose offset is easing (its own builder draws the scrollbar; its children still replay, because the scroll shift is applied at resolve); a text with an unfinished typewriter; the `List` node; and the node the walk must give a pending focus.
4. **Reads of other nodes.** A record notes the ids it read from other nodes (`nav_*` targets, a popup's anchor); if one has changed, the node is dirty.
5. **The environment.** One generation covers what every builder reads: the theme generation (fixed so that loading skins bumps it too), the UI scale and size, the font generation, the prompt context (device, pad family, bindings, icons; it gains a generation), and whether the scene is on top. When it moves, every tree rebuilds once.

**Freshness.** The dirty set is brought up to date before each scene's walk and before each embedded tree is drawn (that tree's nodes only, so many embedded trees cost each tree once), from change ticks since the previous check, so a write made earlier in the same frame (a lower scene's `ui`, or the same `ui` before `ui.node`) is seen that frame, as today. The ticks the cache keeps are clamped on `CheckChangeTicks`, as every engine-held tick is (`src/core/AGENTS.md`, "Change Ticks").

**Propagation.** Each dirty node marks its ancestors up to its root. A node replays only when it and every descendant are clean.

**The spot check (debug builds).** Each frame one clean subtree, chosen round-robin, is built instead of replayed, and its new record is compared with the old one. With unchanged inputs they must be equal; a difference warns once, naming the node and the first differing field. A missed dependency becomes a loud warning rather than a stale pixel. The spot check is counted separately, and is off in `UiHarness` unless a test turns it on.

## Exposure and tooling

- **Automatic.** Every node tree is cached; there is no API change. Game code changes UI as before, by writing components, `ui_set` or `update_ui`. The documented rule: a write through `bypass_change_detection` is invisible to the UI, as it is to any `Changed` query.
- **`debug:ui-rebuilds`** (a console command next to `debug:ui`) outlines each subtree rebuilt this frame, the outline fading over about half a second so steady rebuilds stand out, with a corner counter: rebuilt and replayed nodes this frame.
- **`ui:cache off|on`** (console) and an engine builder option turn caching off. Off is exactly today's path, for comparing output and for ruling caching in or out when chasing a bug.
- **`UiHarness`** gains `set_ui_cache(bool)`, the spot-check switch, and per-frame counts: rebuilt and replayed nodes and which entities rebuilt.
- **No manual invalidation.** Node trees read only components, bindings and the environment, all tracked; a "mark dirty" call would hide a missing dependency instead of fixing it.

## Acceptance

### Correctness

- **The differential oracle** is the main proof. A test helper runs the same scenario in two harnesses, caching on and off, and every frame compares draw commands, interaction records, focus and navigation results, `UiEvent`s and component writes: they must be identical. It runs over:
  - the `ui` demo's node pages (menu, settings, inventory, dialogue, Mixed, Big list) and breakout's menus and HUD;
  - scripted sequences: pointer moves, presses and drags; pad walks and Back; typing in a field; the wheel and the scrollbar; `ui_set`, `update_ui` and a file reload; list count changes, refreshes and jumps; a modal opening and closing; a theme switch, a scale change, a font reload and a pad-family change;
  - a seeded random-input run of a few thousand frames on the settings tree.
- **Rebuild counts:** with no input and nothing live, an unchanged frame rebuilds no node beyond the focused widget's path; hovering a button rebuilds its path on entry and on exit and nothing else; `ui_set` rebuilds only the bound widgets' paths; a theme or scale change rebuilds everything once; an easing scroll area rebuilds itself and replays its rows; a rebound list row rebuilds that row.
- **Pixels:** the demo tour is identical to the pre-4f baseline, with 4e's exclusions (nondeterministic particle and steering pages).

### Benches (`benches/ui.rs`)

- New: "ui frame, the 200-widget settings screen as nodes, unchanged". **Target: under 35 µs** (estimated about 30; today 60).
- New, reported: an unchanged 2,000-node static tree (the cost per replayed node), and the dirty check's own cost.
- The existing node settings frame (focus moving every 10th frame) is reported; the guard (nodes within the larger of 10% or 10 µs of closure) still holds.

### Allocation

An unchanged node frame allocates no more than it does today, and a replay allocates nothing once warm (its own counting-allocator test binary, as `tests/ui_alloc.rs`).

## Documentation

- **`ryo-ui` skill:** a short "Cached node trees" part: what makes a subtree rebuild, the bypass rule, `debug:ui-rebuilds` and `ui:cache`.
- **`src/core/ui/AGENTS.md`:** the record format, the dirtiness sources and the replay rules, with the reason for each.
- **Console help** for the two commands; `ryo-api` for the harness and builder additions.

## The end of 4f: measure again

With caching landed, profile the unchanged and the changing node frames again and write into the backlog note whether layout and paint retention (approach B: skipping clean subtrees in layout, reusing resolved records and commands) is worth a sub-project, with the numbers that decide it. The owner decides.

## Design decisions

- **Equivalence is the spec.** Caching must not be observable: the differential oracle compares caching on and off frame by frame.
- **Relative records.** What a builder pushes is position-independent, so records replay anywhere; context fields are stamped at the splice point.
- **Dirtiness goes up, not across.** Ancestors of a dirty node rebuild their own calls, so a replay never resumes inside a half-open builder.
- **A coarse environment generation.** Theme, scale, font and prompt changes are rare; rebuilding everything once is simple and obviously correct.
- **Records in `UiNodes`, not components.** Inserting a component on the game's entities would change their archetypes, and with them the order of the game's own queries.
- **No manual invalidation.** Every input is tracked; the spot check and the switch find what is not.

## Risks

- **A missed dependency** shows a stale UI, the hardest bug for a game author to see. Mitigated by the oracle, the debug spot check and the switch.
- **Replay cost for tiny subtrees** could approach a rebuild's; measured per node by the 2,000-node bench.
- **The dirty check is a scan** of the UI's entities each pass (fast, but proportional to the UI's size); measured and reported.
- **Records cost memory** about the size of each node's own output.
- **Change-tick wraparound** if a kept tick is not clamped; tested.

## Out of scope

- Layout and paint retention (approach B), left for a measured later sub-project.
- Caching closure UI, which is immediate by design.
- Sharing records between identical subtrees, and building on several threads.
- A manual invalidation API.
