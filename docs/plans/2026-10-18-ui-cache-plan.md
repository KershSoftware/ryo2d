# Cached Node Trees (4f) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make retained node trees incremental: each frame rebuilds only the node subtrees whose inputs changed and replays the rest, invisibly.

**Architecture:** While a node is built, the immediate context records a *tape* of what its builder pushed (nodes, draws, popup brackets, child slots) plus the ids it made and the widget state it touched. The node walk keeps one tape per node in the existing `UiNodes` resource. A dirty set, refreshed from change ticks, removals, interaction roles, live kinds, reads and an environment generation, marks changed nodes and their ancestors; the walk replays a clean subtree's tapes through the same `push_node`/`push_draw`/`close` calls, so every stamp (order, depth, scope, scroll, layer) comes out as a rebuild's would. Layout, resolve, records and draw commands are unchanged.

**Tech Stack:** Rust, bevy_ecs 0.20 (only through `src/core/ecs/kernel.rs`), the engine's UI (`src/core/ui/`).

**Spec:** `docs/specs/2026-10-18-ui-cache-design.md`

## Global Constraints

- Caching is not observable: with caching on and off, every frame's draw commands, interaction records, focus and navigation results, `UiEvent`s and component writes are identical.
- A UI pass adds nothing to a world the game did not ask for: no resource, no component inserted on the game's entities, no bookkeeping entity. Cache state lives in the existing `UiNodes` resource.
- Closure UI is unchanged (immediate by design); only node trees are cached.
- Every tick the cache keeps is clamped on `CheckChangeTicks` (`src/core/AGENTS.md`, "Change Ticks").
- No `unwrap`, `expect` or `panic!` in library code (tests may). bevy_ecs only through `src/core/ecs/kernel.rs`; taffy only in `src/core/ui/layout/`; parley, swash and fdsm only in `src/core/text/`. No new dependencies.
- Bench target: "ui frame, the 200-widget settings screen as nodes, unchanged" under 35 µs. The guard (the node settings frame within the larger of 10% or 10 µs of the closure one) still holds.
- The demo tour is pixel-identical to the pre-4f baseline captured in Task 1 (`/private/tmp/claude-501/4f-tour-before/`), except pages that differ between two runs of the same build (4e found particles 1, 2, 4, 5, 7, 8, 9, 10 and steering 1).
- Skills: `.claude/skills/<n>/SKILL.md` and `src/cli/skills/<n>.md` byte-identical (`cp`, then `cmp`).
- Task order matters: Tasks 3-7 build the mechanism, Task 8's oracle is its proof, and every later task keeps the oracle green.
- Code style: 2-space indent, LF, comments normal and minimal, rustdoc on every public item.
- Commits: `type: brief description` plus bullets; **no trailer or attribution line of any kind** (no `Co-Authored-By`, no `Claude-Session`); never mention AGENTS.md; stage files by name (never `git add -A`, never `data/` or `target/`); no bare `git stash`; do not push.
- The gate before every commit: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`, `cargo test --workspace`, `cargo check --release -p ryo2d --lib`.

## Review Focus

1. **Input a kind consumes without holding a role:** Q/E (`tab_step`) switching a `Tabs` node that is not focused or hovered must still switch with caching on. Test in Task 5.
2. **A drag in progress:** drop targets that are not under the pointer still draw what a drag over the UI makes them draw, and the drag ghost follows the pointer. Test in Task 8 (scenario 10).
3. **Writes made in the same frame before a tree is drawn:** a lower scene's `ui`, or the same `ui` before `ui.node(root, world)`, writing a tree's components; and a bound widget a drag writes, whose sibling bound to the same name is drawn later in the walk. Each shows the new value that frame, as without caching. Tests in Tasks 6, 7 and 10.
4. **Scene transitions:** pushing a scene over a cached tree and popping back (the top flag changes both ways), and a scene that exits (its records dropped). Test in Task 10.
5. **Long sessions:** change ticks wrapping (a kept tick not clamped would make every node look changed, or none). Test in Task 10.

---

## File Map

| File | Task | Responsibility |
|---|---|---|
| `src/core/ui/env.rs` (new) | 1 | `UiEnv`: the generation of everything every builder reads |
| `src/core/ui/theme/mod.rs` | 1 | loading skins bumps the theme generation |
| `src/core/ui/immediate/context.rs` | 1, 3 | the prompt context's generation; `UiContext::env`; recording hooks in `push_node`, `push_draw`, `close`; taint hooks |
| `src/core/ui/tree.rs` | 2 | `Node`, `Draw`, `DrawKind` cloneable; text leaves shared |
| `src/core/ui/tape.rs` (new) | 3 | `Tape`, `Op`, recording and replay on `UiContext` |
| `src/core/ui/id.rs`, `src/core/ui/memory.rs` | 3 | the tape's id log and state-touch log; replaying them |
| `src/core/ui/popup.rs` | 3 | popup brackets on the tape |
| `src/core/ui/nodes/cache.rs` (new) | 4, 5 | `UiCache` (in `UiNodes`): records, the dirty set, the dirty check, roles, live kinds, reads, counters |
| `src/core/ui/nodes/mod.rs` | 4 | `UiNodes.cache` |
| `src/core/engine/frame.rs` | 1, 4, 6, 11 | the env per pass; tick clamping; the switch from `DebugConfig` |
| `src/core/ui/nodes/draw.rs` | 6, 7 | splice or build in the walk |
| `src/core/ui/nodes/bindings.rs` | 6 | writes during the walk mark their targets dirty; `sync_root` reports writes |
| `src/core/ui/nodes/list.rs` | 7 | list rows replay or build |
| `src/core/testing/ui.rs` | 6, 8, 10 | `set_ui_cache`, `ui_cache_stats`, `set_ui_spot_check`, frame snapshots for the oracle |
| `src/core/testing/fixtures.rs` (new) | 8 | the settings node tree shared by tests and benches |
| `src/core/ui/nodes/oracle_tests.rs` (new, `#[cfg(test)]`) | 8 | the differential oracle's core scenarios |
| `examples/demos/ui/tests/cache.rs` (new), `examples/games/breakout/tests/cache.rs` (new) | 9 | the oracle over demo pages and breakout |
| `src/core/ui/debug.rs`, `src/core/console/builtins.rs`, `src/core/config/debug.rs`, `src/core/engine/builder.rs` (or where `EngineBuilder` lives) | 11 | `debug:ui-rebuilds`, `ui:cache`, the builder option |
| `benches/ui.rs`, `tests/ui_cache_alloc.rs` (new) | 12 | benches and the allocation guard |
| skills, `src/core/ui/AGENTS.md`, `docs/architecture/overview.md` | 13 | documentation |
| `docs/backlog/ui-retained-nodes.md` | 14 | the numbers and the recommendation on layout and paint retention |

---

### Task 1: Baseline and the UI environment

**Files:**
- Create: `src/core/ui/env.rs`
- Modify: `src/core/ui/mod.rs` (module), `src/core/ui/theme/mod.rs` (`load_skins`/`refresh_skins` bump the generation), `src/core/ui/immediate/context.rs` (`PromptContext.generation`; `UiContext::env`; the theme generation passed into `UiContext::new`), `src/core/engine/frame.rs` (`refresh_prompts` bumps the prompt generation only on a real change; pass the theme generation), `src/core/testing/ui.rs` (the same)
- Test: `src/core/ui/env.rs` (`mod tests`)

**Interfaces:**
- Produces:
  ```rust
  // src/core/ui/env.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub(crate) struct UiEnv { pub(crate) theme: u64, pub(crate) fonts: u64, pub(crate) scale: u32, pub(crate) size: (u32, u32), pub(crate) prompts: u64 }
  // scale and size are f32 bit patterns (`to_bits`), so equal values compare equal
  impl UiContext<'_> { pub(crate) fn env(&self) -> UiEnv; }
  // PromptContext gains `pub(crate) generation: u64`
  ```

Rules:
- `UiEnv` is everything a builder reads that is not a node's own components: the theme generation (`UiTheme::generation`), the font generation (`TextEngine::generation`), `UiSpace.px_scale` and `UiSpace.size`, and the prompt context's generation.
- Loading skins (`refresh_skins`/`load_skins`) changes what is drawn, so it bumps the theme generation like `set`/`modify` do.
- `refresh_prompts` keeps the device, pad family, bindings and icons it built last; it bumps `PromptContext.generation` only when one of them differs.

- [ ] **Step 1: Capture the baseline tour** before any change: `./dev.sh demos:tour`, then copy `target/demo-tour/` to `/private/tmp/claude-501/4f-tour-before/`. Capture it a second time into `/private/tmp/claude-501/4f-tour-before-2/` and list the pages that differ between the two (`compare -metric AE a.png b.png null:`, stderr): those are nondeterministic and excluded from every later comparison. Write the list into the task report.

- [ ] **Step 2: Write the failing tests** in `src/core/ui/env.rs`:

```rust
#[cfg(test)]
mod tests {
  use crate::core::testing::UiHarness;
  use crate::prelude::*;

  fn env(h: &mut UiHarness) -> crate::core::ui::env::UiEnv {
    h.frame(|ui| ui.env())
  }

  #[test]
  fn the_env_is_stable_while_nothing_changes() {
    let mut h = UiHarness::new(640.0, 480.0);
    let first = env(&mut h);
    assert_eq!(env(&mut h), first);
  }

  #[test]
  fn a_theme_change_moves_the_env() {
    let mut h = UiHarness::new(640.0, 480.0);
    let before = env(&mut h);
    h.set_theme(Theme::light());
    assert_ne!(env(&mut h), before);
  }

  #[test]
  fn a_scale_or_size_change_moves_the_env() {
    let mut h = UiHarness::new(640.0, 480.0);
    let before = env(&mut h);
    h.resize(800.0, 600.0);
    assert_ne!(env(&mut h), before);
  }

  #[test]
  fn loading_skins_bumps_the_theme_generation() {
    let mut theme = crate::core::ui::theme::UiTheme::default();
    let before = theme.generation();
    theme.load_skins(|_| None);
    assert_ne!(theme.generation(), before, "skins can change what is drawn");
  }
}
```

(If `UiHarness` has no `set_theme`/`resize`, add them as small public test helpers next to `with_theme`: `set_theme` replaces the harness theme for the next frame; `resize` changes the UI size. If `Theme::light()` is not a constructor, use any second theme the theme tests already build, and say so in the report. Add a prompt-generation test in `context.rs`'s or `frame.rs`'s tests: building the prompt context twice from the same input leaves `generation` unchanged; changing the last device bumps it.)

- [ ] **Step 3: Run them to see them fail** — `cargo test -p ryo2d --lib core::ui::env`: compile errors (no `env`, no `UiEnv`).

- [ ] **Step 4: Implement** by the rules.

- [ ] **Step 5: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes.

- [ ] **Step 6: Run the gate and commit**:

```bash
git add src/core/ui/env.rs src/core/ui/mod.rs src/core/ui/theme/mod.rs src/core/ui/immediate/context.rs src/core/engine/frame.rs src/core/testing/ui.rs
git commit -m "feat: one generation for what every UI builder reads

- Theme, fonts, scale, size and prompts in a UiEnv
- Loading skins bumps the theme generation; prompts bump theirs only on a change"
```

---

### Task 2: Cloneable nodes and shared text

**Files:**
- Modify: `src/core/ui/tree.rs` (`Node`, `Draw`, `DrawKind` derive `Clone`; `Node.text: Option<Arc<TextLeaf>>`; `DrawKind::Text { leaf: Arc<TextLeaf>, .. }`), `src/core/ui/immediate/context.rs` (`push_draw` resolves prompts with `Arc::make_mut` only when the leaf has prompts; `text_leaf`/`leaf_of` callers), every widget that builds a `DrawKind::Text` or sets `Node.text` (grep `DrawKind::Text {` and `.text = Some(`)
- Test: `src/core/ui/tree.rs` (`mod tests`)

**Interfaces:**
- Produces: `impl Clone for Node, Draw, DrawKind`; text leaves behind `Arc`, so cloning a node or a draw never copies text.

Rules:
- No behaviour change: the demo tour stays pixel-identical (Task 9 checks it; run the `text` and `ui` demo pages' captures here too).
- Cloning a `Node` or a `Draw` allocates nothing: every heap payload is behind an `Arc` (`TextLeaf`; `DrawKind::Local(UiCommand)` if `UiCommand` owns a heap payload — check it and wrap the heavy variant if so). `UiStyle` may hold `Tracks::Fixed(Vec<f32>)`: grids with fixed tracks clone a small vector; leave it and note it in the report.
- `push_draw` keeps its prompt resolution: a leaf with prompts is resolved into a new leaf (`Arc::make_mut` clones it once); a leaf without prompts is pushed as is.

- [ ] **Step 1: Write the failing tests** in `src/core/ui/tree.rs`:

```rust
#[cfg(test)]
mod clone_tests {
  use super::*;
  use std::sync::Arc;

  #[test]
  fn a_cloned_text_draw_shares_its_leaf() {
    let leaf = Arc::new(TextLeaf::new(RichText::plain("Hello"), TextStyle::default(), TextLayoutOptions::default()));
    let draw = Draw {
      order: 3,
      layer: 1,
      node: 0,
      kind: DrawKind::Text { leaf: Arc::clone(&leaf), frac: Vec2::ZERO, wrap: false, opacity: 1.0, reveal: None },
    };
    let copy = draw.clone();
    let DrawKind::Text { leaf: copied, .. } = &copy.kind else { panic!("a text draw") };
    assert!(Arc::ptr_eq(copied, &leaf));
    assert_eq!((copy.order, copy.layer, copy.node), (3, 1, 0));
  }

  #[test]
  fn a_cloned_node_shares_its_text() {
    let mut node = Node::new(UiId::NONE, UiStyle::default(), Measure::None);
    node.text = Some(Arc::new(TextLeaf::new(RichText::plain("Label"), TextStyle::default(), TextLayoutOptions::default())));
    let copy = node.clone();
    assert!(Arc::ptr_eq(copy.text.as_ref().unwrap(), node.text.as_ref().unwrap()));
  }
}
```

(Use the real constructors for `RichText`, `TextStyle` and `TextLayoutOptions` the existing tree tests use; adjust imports to what compiles.)

- [ ] **Step 2: Run them to see them fail** — compile errors (`Draw`/`Node` not `Clone`; `leaf` is a `Box`).
- [ ] **Step 3: Implement** by the rules.
- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` and `cargo test -p ryo2d --lib core::text` pass. Capture `./dev.sh demos:tour text` and `./dev.sh demos:tour ui`, compare every page with the baseline: AE 0 (bar the excluded pages).
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/tree.rs src/core/ui/immediate/context.rs <every widget file changed>
git commit -m "refactor: UI nodes and draws clone without copying text

- Text leaves are shared behind an Arc; Node, Draw and DrawKind are Clone"
```

---

### Task 3: The tape

**Files:**
- Create: `src/core/ui/tape.rs`
- Modify: `src/core/ui/mod.rs` (module), `src/core/ui/immediate/context.rs` (the recording stack field; hooks in `push_node`, `push_draw`, `close`; `taint` calls), `src/core/ui/id.rs` (an id log for tapes), `src/core/ui/memory.rs` (a state-touch log for tapes; `touch(id, scene)`), `src/core/ui/popup.rs` (popup brackets recorded), the side-effect sites listed below (`taint`)
- Test: `src/core/ui/tape.rs` (`mod tests`)

**Interfaces:**
- Consumes: Task 2's `Clone` for `Node`, `Draw`, `DrawKind`.
- Produces:
  ```rust
  // src/core/ui/tape.rs
  #[derive(Clone, Default)]
  pub(crate) struct Tape {
    pub(crate) ops: Vec<Op>,
    pub(crate) ids: Vec<UiId>,      // ids made by this node's own builder call, in order
    pub(crate) states: Vec<UiId>,   // widget-state ids it touched, in order
    pub(crate) replayable: bool,    // false when its build had a side effect (see taint)
  }
  #[derive(Clone)]
  pub(crate) enum Op {
    Node { node: Node, at: Option<Rect> }, // as passed to push_node, before stamping
    Close(u32),                            // the k-th node this tape pushed
    Draw { node: u32, kind: DrawKind },    // on the k-th node this tape pushed, as passed to push_draw
    PopupBegin { anchor: UiId, modal: bool }, // anchor: the id whose last-frame rect places it (NONE: none)
    PopupEnd,
    Child(Entity),                         // where a child's output goes
  }
  impl UiContext<'_> {
    pub(crate) fn start_tape(&mut self);
    pub(crate) fn end_tape(&mut self) -> Tape;
    pub(crate) fn begin_child(&mut self, child: Entity); // records Op::Child, pauses the open tape
    pub(crate) fn end_child(&mut self);
    pub(crate) fn taint(&mut self);                      // the open tape (and every tape below it) is not replayable
    pub(crate) fn replay(&mut self, tape: &Tape, child: &mut dyn FnMut(&mut UiContext<'_>, Entity));
  }
  // id.rs: IdScopes logs made ids for the open tape; `replay_id(id)` inserts into `used` and pushes to `captured` when capturing
  // memory.rs: UiMemory logs `state()` touches for the open tape; `touch(id, scene)` stamps a slot seen
  ```

Rules:
- **Recording.** `start_tape` pushes a recorder. While a recorder is open and not paused, `push_node` appends `Op::Node` (a clone of the node as passed, before stamping) and remembers the subtree index it returned, as the tape-local index `k`; `push_draw` appends `Op::Draw` with its node mapped to `k`; `close` appends `Op::Close(k)`; `begin_popup`/`end_popup` append `PopupBegin`/`PopupEnd` (the popup's anchor *id*, so a replay looks its rect up again as the builder does: find where `ui.popup(anchor)` turns the id into a rect and record the id); `scopes.make` logs the id; `memory.state` logs the touch. A draw on a node this tape did not push, or a `close` of one, taints the tape.
- **Children.** `begin_child(e)` appends `Op::Child(e)` to the open tape and pauses it; pushes, ids and touches made until `end_child` belong to the child (its own tape, or its replay), not the parent's. Pauses nest.
- **Taint.** A build that does anything a replay would not repeat taints its tape and every open tape below it: setting `pending_tooltip`, `request_focus`/`request_scroll`/`release_focus`, consuming input (`take_tab_step`, `consume_keyboard` and the like: grep `fn take_`/`consume` in `immediate/` and `widgets/`), playing a sound, queuing a rebind, `ui.back()`, a drag ghost, an absolute `draw_*`/`record_*` command, and `ui.node(..)` of an embedded tree (until Task 7 handles it). A tainted tape is recorded but `replayable` is false.
- **Replay.** `replay` walks `ops`: `Node` → `push_node(node.clone(), at)` (so order, layer, scope, parent, depth and scroll are stamped from the open context, as in a build) and remember its new index for `k`; `Draw` → `push_draw(index_of(k), kind.clone())`; `Close(k)` → `close_node(index_of(k))`; `PopupBegin` → `begin_popup(rect of anchor as the builder computes it, modal)`; `PopupEnd` → `end_popup()`; `Child(e)` → `child(self, e)`. Then each id in `ids` → `scopes.replay_id`, each state in `states` → `memory.touch(id, scene)`. A replay records into an open tape only through `Child` (a parent never sees its children's ops).
- No allocation per replayed op once warm, beyond what `push_node`/`push_draw` do with the cloned values (Task 2 made those clones allocation-free).

- [ ] **Step 1: Write the failing tests** in `src/core/ui/tape.rs`. They tape closure-built subtrees and compare a replay with a build of the same code, frame for frame:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::prelude::*;

  fn panel(ui: &mut UiContext<'_>) {
    ui.column().id("panel").gap(4.0).padding(8.0).show(|ui| {
      ui.text("Title").show();
      ui.button("Play").id("play").show();
      ui.row().id("pair").gap(2.0).show(|ui| {
        ui.button("A").id("a").show();
        ui.button("B").id("b").show();
      });
    });
  }

  /// The frame's draw commands and the records of `ids`, to compare two frames
  fn snapshot(h: &UiHarness, ids: &[UiId]) -> String {
    let records: Vec<String> = ids.iter().map(|id| format!("{:?}", h.record_of(*id))).collect();
    format!("{:?}\n{:?}", h.commands_debug(), records)
  }

  #[test]
  fn a_replayed_subtree_draws_what_a_build_draws() {
    let mut built = UiHarness::new(640.0, 480.0);
    let mut tape = Tape::default();
    built.frame(|ui| { ui.start_tape(); panel(ui); tape = ui.end_tape(); });
    built.frame(panel);
    let ids = built.frame(|ui| { panel(ui); [ui.id_of("panel"), ui.id_of("play"), ui.id_of("a")] });

    let mut replayed = UiHarness::new(640.0, 480.0);
    replayed.frame(panel);
    replayed.frame(panel);
    replayed.frame(|ui| ui.replay(&tape, &mut |_, _| {}));
    assert!(tape.replayable);
    assert_eq!(snapshot(&replayed, &ids), snapshot(&built, &ids));
  }

  #[test]
  fn a_replay_takes_the_orders_a_build_would() {
    let mut h = UiHarness::new(640.0, 480.0);
    let mut tape = Tape::default();
    let before = h.frame(|ui| { ui.start_tape(); panel(ui); tape = ui.end_tape(); ui.next_order() });
    let after = h.frame(|ui| { ui.replay(&tape, &mut |_, _| {}); ui.next_order() });
    assert_eq!(after, before);
  }

  #[test]
  fn a_replay_keeps_the_state_it_touched_and_registers_its_ids() {
    let mut h = UiHarness::new(640.0, 480.0);
    let mut tape = Tape::default();
    let scroll = h.frame(|ui| {
      ui.start_tape();
      let r = ui.scroll().id("s").height(100.0).show(|ui| { for i in 0..20 { ui.button(format!("{i}")).id(i).show(); } });
      tape = ui.end_tape();
      r.id
    });
    h.frame(|ui| ui.set_scroll_offset(scroll, Vec2::new(0.0, 40.0)));
    for _ in 0..3 {
      h.frame(|ui| {
        ui.replay(&tape, &mut |_, _| {});
        assert!(ui.memory.scopes.made(scroll), "the replayed ids are registered");
      });
    }
    assert!((h.scroll_offset(scroll).y - 40.0).abs() < 0.5, "the scroll state survived the replays");
  }

  #[test]
  fn a_child_slot_is_filled_by_its_own_output() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (mut outer, mut inner) = (Tape::default(), Tape::default());
    let child = Entity::from_raw_u32(7).unwrap();
    h.frame(|ui| {
      ui.start_tape();
      ui.column().id("outer").show(|ui| {
        ui.begin_child(child);
        ui.start_tape();
        ui.button("Inner").id("inner").show();
        inner = ui.end_tape();
        ui.end_child();
      });
      outer = ui.end_tape();
    });
    assert!(outer.ops.iter().any(|op| matches!(op, Op::Child(e) if *e == child)));
    assert!(!outer.ops.iter().any(|op| matches!(op, Op::Node { node, .. } if node.id == inner_id(&inner))),
      "the parent's tape does not hold its child's nodes");
    let count = h.frame(|ui| { ui.replay(&outer, &mut |ui, _| ui.replay(&inner, &mut |_, _| {})); ui.next_order() });
    assert!(count > 0);
  }

  fn inner_id(tape: &Tape) -> UiId {
    tape.ops.iter().find_map(|op| match op { Op::Node { node, .. } => Some(node.id), _ => None }).unwrap_or(UiId::NONE)
  }

  #[test]
  fn a_build_with_a_side_effect_is_not_replayable() {
    let mut h = UiHarness::new(640.0, 480.0);
    let mut tape = Tape::default();
    h.frame(|ui| {
      ui.start_tape();
      ui.column().id("c").show(|ui| { let id = ui.id_of("x"); ui.request_focus(id); });
      tape = ui.end_tape();
    });
    assert!(!tape.replayable);
  }

  #[test]
  fn a_popup_replays_into_the_popup_queue() {
    let mut built = UiHarness::new(640.0, 480.0);
    let mut tape = Tape::default();
    let menu = |ui: &mut UiContext<'_>| { ui.modal().id("m").show(|ui| { ui.button("Ok").id("ok").show(); }); };
    built.frame(|ui| { ui.start_tape(); menu(ui); tape = ui.end_tape(); });
    let ok = built.frame(|ui| { menu(ui); ui.id_of("ok") });
    let mut replayed = UiHarness::new(640.0, 480.0);
    replayed.frame(menu);
    replayed.frame(|ui| ui.replay(&tape, &mut |_, _| {}));
    assert!(tape.replayable);
    assert_eq!(snapshot(&replayed, &[ok]), snapshot(&built, &[ok]));
  }
}
```

(Add the crate test helpers the tests use if missing: `UiHarness::commands_debug() -> String` (the frame's `UiCommands` in `Debug` form, `#[cfg(test)]`), and `UiContext::id_of(key) -> UiId` (the id a keyed widget at the call site would get: if the keyed id depends on the call site, compute ids by drawing and reading `Response.id` instead, and adjust the tests). Fix any other helper name to what exists, and say so.)

- [ ] **Step 2: Run them to see them fail** — `cargo test -p ryo2d --lib core::ui::tape`: compile errors.
- [ ] **Step 3: Implement** by the rules.
- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes (closure UI is untouched when no tape is open: the existing tests are the guard).
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/tape.rs src/core/ui/mod.rs src/core/ui/immediate/context.rs src/core/ui/id.rs src/core/ui/memory.rs src/core/ui/popup.rs src/core/testing/ui.rs <taint sites changed>
git commit -m "feat: record a UI builder's output as a tape and replay it

- Nodes, draws, popup brackets and child slots, replayed through push_node so every stamp matches a build
- Ids and widget-state touches replayed; a build with a side effect is never replayed"
```

---

### Task 4: The dirty check

**Files:**
- Create: `src/core/ui/nodes/cache.rs`
- Modify: `src/core/ui/nodes/mod.rs` (`UiNodes.cache: UiCache`), `src/core/engine/frame.rs` (clamp the cache's ticks where the engine handles `CheckChangeTicks`), `src/core/testing/ui.rs` (the same clamp path, callable from tests)
- Test: `src/core/ui/nodes/cache.rs` (`mod tests`)

**Interfaces:**
- Produces:
  ```rust
  // src/core/ui/nodes/cache.rs
  #[derive(Default)]
  pub(crate) struct UiCache {
    pub(crate) enabled: bool,                    // true by default (Default must give true: implement Default by hand)
    pub(crate) records: EntityHashMap<NodeRecord>,
    pub(crate) dirty: EntityHashSet,             // changed nodes and their ancestors, until rebuilt
    pub(crate) checked: Option<Tick>,            // the change tick of the last check
    pub(crate) stats: CacheStats,
    // the queries and removal cursors the check uses (built lazily, rebuilt when the WorldId differs)
  }
  pub(crate) struct NodeRecord {
    pub(crate) tape: Tape, pub(crate) scene: u64, pub(crate) env: UiEnv, pub(crate) top: bool, pub(crate) frame: u64,
    pub(crate) id: UiId,    // the node's response id: what a replay returns, with last frame's rect
    pub(crate) dialog: bool, // a modal or popup container: the walk's Back target
  }
  #[derive(Debug, Clone, Default, PartialEq, Eq)]
  pub(crate) struct CacheStats { pub(crate) built: usize, pub(crate) replayed: usize, pub(crate) rebuilt: Vec<Entity> }
  pub(crate) fn refresh_dirty(world: &mut World);      // brings `dirty` up to date from change ticks and removals
  pub(crate) fn mark_dirty(world: &mut World, node: Entity); // marks `node` and its ancestors (ChildOf) up to a root
  pub(crate) fn check_ticks(cache: &mut UiCache, check: CheckChangeTicks);
  ```

Rules:
- **Tracked components:** `UiNodeKind`, `UiRoot`, `UiLabel`, `UiValue`, `UiChecked`, `UiIndex`, `UiOptions`, `UiTextValue`, `UiSlot`, `UiImage`, `UiTooltip`, `UiHidden`, `UiDisabled`, `UiBind`, `UiPattern`, `TextReveal`, `UiListRow`, `Children`, `ChildOf`. Confirm the list against what `NodeRead` and every `draw_*` in `nodes/draw.rs` read (grep `get::<` there) and add any component a builder reads; say what you added.
- **Changes:** one query over UI node entities (`With<UiNodeKind>`) filtered by `Or<(Changed<..>, ..)>` of the tracked components (nest `Or` tuples past bevy's arity), iterated with the cache's own last tick: `world.last_change_tick_scope(checked, |world| query.iter_manual(world))` (bevy 0.20's safe route; the engine clears trackers in `Last`, after the UI pass, so the world's own last tick would miss the previous frame's `update`). After the scan, `checked = world.read_change_tick()`. The first check (no `checked`) marks nothing: every node without a record is built anyway.
- **Removals:** a removed `UiHidden`, `UiDisabled`, `UiTooltip` or `UiBind` (and a despawned child, which changes its parent's `Children`) marks the entity: read `world.removed_components()` for each type through a `MessageCursor` kept in the cache (removal messages last two tracker clears, so a cursor read every pass misses none).
- **Propagation:** `mark_dirty` inserts the node and walks `ChildOf` upward, inserting each ancestor, stopping at an entity already in `dirty` (its ancestors are marked) or at a root.
- **Storage:** the queries and cursors live in `UiCache` (inside `UiNodes`, which exists only once the game spawned UI): the check adds no resource. Take `UiNodes` out with the existing `resource_scope` pattern while iterating.
- **Ticks:** `check_ticks` clamps `checked` with `Tick::check_tick`; call it wherever the engine handles `CheckChangeTicks` for its own ticks (`clamp_schedule_ticks` or the `PendingTickCheck` path in `engine/frame.rs` / `engine/mod.rs`), and from `UiHarness` when a test asks.
- `refresh_dirty` returns at once in a world without `UiNodes`.

- [ ] **Step 1: Write the failing tests** in `src/core/ui/nodes/cache.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  fn tree(h: &mut UiHarness) -> (Entity, Entity, Entity) {
    let root = h.spawn_ui(UiNode::column().name("root").child(UiNode::row().name("row").child(UiNode::button("Go").name("go"))));
    h.frame(|_| ());
    let row = h.find_ui("row").unwrap();
    let go = h.find_ui("go").unwrap();
    (root, row, go)
  }

  fn dirty(h: &mut UiHarness) -> EntityHashSet {
    refresh_dirty(h.world_mut());
    h.world().get_resource::<UiNodes>().map(|n| n.cache.dirty.clone()).unwrap_or_default()
  }

  fn clear(h: &mut UiHarness) {
    refresh_dirty(h.world_mut());
    if let Some(mut n) = h.world_mut().get_resource_mut::<UiNodes>() { n.cache.dirty.clear(); }
  }

  #[test]
  fn a_changed_label_marks_its_node_and_ancestors() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (root, row, go) = tree(&mut h);
    clear(&mut h);
    h.world_mut().get_mut::<UiLabel>(go).unwrap().0 = "Stop".into();
    let d = dirty(&mut h);
    assert!(d.contains(&go) && d.contains(&row) && d.contains(&root));
  }

  #[test]
  fn nothing_changed_marks_nothing() {
    let mut h = UiHarness::new(640.0, 480.0);
    tree(&mut h);
    clear(&mut h);
    assert!(dirty(&mut h).is_empty());
  }

  #[test]
  fn a_change_made_in_the_previous_update_is_seen_after_trackers_clear() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (_, _, go) = tree(&mut h);
    clear(&mut h);
    h.world_mut().get_mut::<UiLabel>(go).unwrap().0 = "Later".into();
    h.world_mut().clear_trackers(); // the engine clears in Last, after the UI pass
    assert!(dirty(&mut h).contains(&go));
  }

  #[test]
  fn a_removed_component_marks_its_node() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (_, _, go) = tree(&mut h);
    h.world_mut().entity_mut(go).insert(UiDisabled);
    clear(&mut h);
    h.world_mut().entity_mut(go).remove::<UiDisabled>();
    assert!(dirty(&mut h).contains(&go));
  }

  #[test]
  fn a_despawned_child_marks_its_parent() {
    let mut h = UiHarness::new(640.0, 480.0);
    let (_, row, go) = tree(&mut h);
    clear(&mut h);
    h.world_mut().despawn(go);
    assert!(dirty(&mut h).contains(&row));
  }

  #[test]
  fn a_world_without_ui_is_left_alone() {
    let mut world = World::new();
    let before = world.entities().len();
    refresh_dirty(&mut world);
    assert_eq!(world.entities().len(), before);
    assert!(world.get_resource::<UiNodes>().is_none());
  }
}
```

(Use the kernel's names: `EntityHashSet` is `pub(crate)` there. If `UiDisabled`/`UiLabel` construct differently, use the real constructors.)

- [ ] **Step 2: Run them to see them fail** — compile errors.
- [ ] **Step 3: Implement** by the rules.
- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui::nodes` passes.
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/nodes/cache.rs src/core/ui/nodes/mod.rs src/core/engine/frame.rs src/core/testing/ui.rs
git commit -m "feat: track which UI nodes changed since they were built

- Change ticks and removals of the UI's components, kept by the cache's own tick
- A changed node marks its ancestors; the kept tick is clamped with the engine's"
```

---

### Task 5: Interaction, live kinds, reads and the environment

**Files:**
- Modify: `src/core/ui/nodes/cache.rs` (roles, live set, reads, env checks), `src/core/ui/memory.rs` (`Interaction::roles`), `src/core/ui/tape.rs` (a tape notes what it read from other nodes)
- Test: `src/core/ui/nodes/cache.rs` (`mod live_tests`)

**Interfaces:**
- Consumes: Task 1's `UiEnv`; Task 4's `UiCache`, `mark_dirty`.
- Produces:
  ```rust
  impl Interaction { pub(crate) fn roles(&self) -> Roles; } // every id holding a role this frame
  pub(crate) struct Roles([UiId; 20]); // a fixed array (NONE for an empty slot): no heap, no new dependency
  // UiCache gains: `by_id: FastMap<UiId, Entity>` (filled when a record is stored: each node id the tape pushed -> the entity),
  //   `last_roles: Roles`, `live: EntityHashSet`
  // Tape gains: `pub(crate) reads: Vec<(UiId, Option<Rect>)>` (an id read from another node, and the rect when the build placed by it)
  pub(crate) fn mark_live(world: &mut World, ui: &UiContext<'_>, scene: u64); // roles (now and last), live kinds, tab_step, drag
  pub(crate) fn note_live(cache: &mut UiCache, node: Entity, kind: &UiNodeKind); // called when a record is stored: always-live kinds go into `live`
  pub(crate) fn clean(world: &World, ui: &UiContext<'_>, node: Entity) -> bool; // a record, replayable, same env and top, reads unchanged, not dirty
  ```

Rules:
- **Roles:** hot, hovered, active, clicked, secondary, double, drag_started, drag_released, press_target, focused, gained, lost, activated, `nav_step`'s id, drop_hover, `dropped`'s id and the drag source's id (`DragState`). `mark_live` marks the entity of every id in this frame's roles **or** last frame's (`by_id`), then stores this frame's as `last_roles`.
- **Live kinds:** a `Kind::Scroll` or `Kind::List` node always rebuilds its own call (its children may still replay); a `Kind::Text` whose `TextReveal` is unfinished; the walk's pending-focus node; while `tab_step != 0`, every `Kind::Tabs` node of the top scene; while a drag is in progress (`Interaction::drag` is `Some`), every node with the drop-target flag. Keep the always-live nodes in `live` (filled as records are stored: the kind is known then) so marking them costs their count, not a scan.
- **Reads:** while taping, `recorded_id` (`nav_*` neighbours) and the popup anchor lookup note `(id read, rect used)` on the open tape. `clean` compares them with the current values (the neighbour entity's `UiInteraction.id`; the anchor's last-frame record rect); a difference makes the node dirty.
- **Environment:** a record whose `env` differs from `ui.env()`, or whose `top` differs from the scene's top flag, is not clean (the whole tree rebuilds once, since every node's record is stale).
- `clean(node)` is: caching enabled, a record exists, `tape.replayable`, same env and top, reads unchanged, not in `dirty`.

- [ ] **Step 1: Write the failing tests** in `src/core/ui/nodes/cache.rs` (`mod live_tests`). The walk does not store records until Task 6, so these tests fill the cache by hand (`by_id`, a record, `note_live`) where they need one:

```rust
#[cfg(test)]
mod live_tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::id::UiId;
  use crate::core::ui::memory::Interaction;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  #[test]
  fn roles_hold_every_id_with_a_part_this_frame() {
    let mut i = Interaction::default();
    i.hovered = UiId(11);
    i.focused = UiId(12);
    i.clicked = UiId(13);
    i.drop_hover = UiId(14);
    let roles = i.roles();
    for id in [11, 12, 13, 14] {
      assert!(roles.contains(UiId(id)), "{id}");
    }
    assert!(!roles.contains(UiId(99)));
  }

  fn cache_mut(h: &mut UiHarness) -> Mut<'_, UiNodes> {
    h.world_mut().resource_mut::<UiNodes>()
  }

  #[test]
  fn the_roles_of_this_frame_and_the_last_are_marked() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(UiNode::column().focus_scope().child(UiNode::button("A").name("a").default_focus()).child(UiNode::button("B").name("b")));
    for _ in 0..3 { h.frame(|_| ()); }
    let (a, b) = (h.find_ui("a").unwrap(), h.find_ui("b").unwrap());
    let (ida, idb) = (h.ui_id(a).unwrap(), h.ui_id(b).unwrap());
    cache_mut(&mut h).cache.by_id.extend([(ida, a), (idb, b)]);
    // A holds the focus: marking now records it as last frame's
    h.frame_with_world(|ui, world| mark_live(world, ui, ui.scene().0));
    cache_mut(&mut h).cache.dirty.clear();
    h.action("ui.down"); // the focus moves from A to B
    let marked = h.frame_with_world(|ui, world| {
      mark_live(world, ui, ui.scene().0);
      world.resource::<UiNodes>().cache.dirty.clone()
    });
    assert!(marked.contains(&a), "A lost the focus: it draws its exit");
    assert!(marked.contains(&b), "B took it");
  }

  #[test]
  fn tab_step_marks_tabs_that_hold_no_role() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(UiNode::column().focus_scope().child(UiNode::tabs(&["One", "Two"], 0).name("tabs")).child(UiNode::button("Other").name("other").default_focus()));
    for _ in 0..3 { h.frame(|_| ()); }
    let tabs = h.find_ui("tabs").unwrap();
    let kind = h.world().get::<UiNodeKind>(tabs).unwrap().clone();
    note_live(&mut cache_mut(&mut h).cache, tabs, &kind);
    h.action("ui.tab_next"); // use 4b's real action name for Q/E
    let marked = h.frame_with_world(|ui, world| {
      mark_live(world, ui, ui.scene().0);
      world.resource::<UiNodes>().cache.dirty.clone()
    });
    assert!(marked.contains(&tabs));
  }

  #[test]
  fn a_changed_env_makes_a_record_unclean() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(UiNode::column().child(UiNode::button("A").name("a")));
    for _ in 0..3 { h.frame(|_| ()); }
    let a = h.find_ui("a").unwrap();
    let (env, scene, frame) = h.frame(|ui| (ui.env(), ui.scene().0, ui.memory.frame()));
    let record = NodeRecord { tape: Tape { replayable: true, ..Tape::default() }, scene, env, top: true, frame, id: UiId::NONE, dialog: false };
    cache_mut(&mut h).cache.records.insert(a, record);
    assert!(h.frame_with_world(|ui, world| clean(world, ui, a)), "same env: clean");
    h.set_theme(Theme::light()); // any second theme, as in Task 1
    assert!(!h.frame_with_world(|ui, world| clean(world, ui, a)), "another theme: stale");
  }
}
```

(`note_live(cache, entity, kind)` is the function Task 6 calls when it stores a record: it puts always-live kinds into `live`. If `frame()` ends the frame before `clean` runs and drops the hand-made record (Task 6 adds that drop), re-insert it inside the same `frame_with_world` call.)

- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement** by the rules.
- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes (or the `#[ignore]`d ones are left for Task 6).
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/nodes/cache.rs src/core/ui/memory.rs src/core/ui/tape.rs
git commit -m "feat: what else makes a cached UI node rebuild

- Interaction roles this frame and last, live kinds, reads of other nodes
- A changed environment or top flag makes every record stale"
```

---

### Task 6: Splice or build

**Files:**
- Modify: `src/core/ui/nodes/draw.rs` (`draw_nodes` refreshes the dirty set after syncing the scene's roots; `draw_node` replays a clean node or builds and records it; `walk_children` brackets each child with `begin_child`/`end_child`; the Back target and pending focus), `src/core/ui/nodes/bindings.rs` (`sync_root` returns whether it wrote; a write the walk makes through bindings marks its target dirty), `src/core/ui/nodes/cache.rs` (storing records, dropping undrawn ones, stats), `src/core/testing/ui.rs` (`set_ui_cache`, `ui_cache_stats`), `src/core/engine/frame.rs` (the end-of-frame record drop; the switch read from `DebugConfig.ui_cache` once Task 9 adds it — until then, `UiCache.enabled` stays true)
- Test: `src/core/ui/nodes/draw.rs` (`mod cache_tests`)

**Interfaces:**
- Consumes: Tasks 3-5.
- Produces:
  ```rust
  impl UiHarness {
    pub fn set_ui_cache(&mut self, on: bool);
    pub fn ui_cache_stats(&self) -> UiCacheStats; // public mirror of CacheStats: built, replayed, rebuilt entities (last frame)
  }
  #[derive(Debug, Clone, Default, PartialEq, Eq)] pub struct UiCacheStats { pub built: usize, pub replayed: usize, pub rebuilt: Vec<Entity> }
  ```

Rules:
- **Order in `draw_nodes`:** sync every root of the scene (`bindings::sync_root`), then `refresh_dirty`, then `mark_live`, then walk the roots.
- **`draw_node(node)`:** if `clean(node)` → replay its record's tape with a child callback that calls `draw_node(child)` for each `Child(e)` (children decide for themselves: a clean parent only has clean children, since dirtiness goes up); restore `walk.back = Some(node)` if the record was a dialog; count `replayed += nodes in the tape`; return the record's last `Response` (id and last-frame rect; store `id` in the record) so callers that use it still get one. Else → `start_tape`, build as today with `walk_children` calling `begin_child(child)` / `draw_node(child)` / `end_child()` around each child, `end_tape` after `answer`/`answer_container`, store the record (tape, scene, env, top, frame, the `Response` id), remove the node from `dirty`, update `by_id` and `live`, count `built += 1` and push to `rebuilt`.
- **Writes during the walk:** a component a built node writes to **another** node (`widget_wrote` → `bindings::propagate`) marks the written entity dirty at once (`mark_dirty`), so a node later in the same walk that shows it is built, not replayed (Review Focus 3). A node's writes to its own components (`Write::apply`, `TextReveal`, `UiTextValue`) do not mark it: it was just built from them. After the scene's walk, `checked` is advanced to `world.read_change_tick()` so none of the walk's writes is counted again next frame (they are already in `dirty` where they matter). Nothing else runs during a walk: events are delivered, and observers run, after it, so their writes are seen next frame as before.
- **Records not drawn this frame** are dropped at the end of the frame (`NodeRecord.frame != memory.frame()`), as widget state is; so are the records of an exited scene.
- **The switch:** `UiCache.enabled == false` builds every node as today and stores no record. `UiHarness::set_ui_cache` sets it.
- Embedded trees and list rows are not replayed yet: `ui.node(..)` taints (Task 3), and `Pool::contents` builds rows as today (Task 7).

- [ ] **Step 1: Write the failing tests** in `src/core/ui/nodes/draw.rs` (`mod cache_tests`):

```rust
#[cfg(test)]
mod cache_tests {
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  fn menu() -> UiNode {
    UiNode::column().focus_scope().gap(4.0)
      .child(UiNode::text("Menu").name("title"))
      .child(UiNode::button("Play").name("play"))
      .child(UiNode::row().name("pair").child(UiNode::button("A").name("a")).child(UiNode::button("B").name("b")))
  }

  fn warm(h: &mut UiHarness) { for _ in 0..3 { h.frame(|_| ()); } }

  #[test]
  fn an_unchanged_frame_builds_nothing() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(menu());
    warm(&mut h);
    h.frame(|_| ());
    let stats = h.ui_cache_stats();
    assert_eq!(stats.built, 0, "rebuilt: {:?}", stats.rebuilt);
    assert!(stats.replayed > 0);
  }

  #[test]
  fn a_changed_label_rebuilds_its_path_only() {
    let mut h = UiHarness::new(640.0, 480.0);
    let root = h.spawn_ui(menu());
    warm(&mut h);
    let a = h.find_ui("a").unwrap();
    h.world_mut().get_mut::<UiLabel>(a).unwrap().0 = "Alpha".into();
    h.frame(|_| ());
    let rebuilt = h.ui_cache_stats().rebuilt;
    let pair = h.find_ui("pair").unwrap();
    assert_eq!(rebuilt.len(), 3, "{rebuilt:?}");
    assert!(rebuilt.contains(&a) && rebuilt.contains(&pair) && rebuilt.contains(&root));
  }

  #[test]
  fn hovering_a_button_rebuilds_it_on_entry_and_on_exit() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(menu());
    warm(&mut h);
    let play = h.find_ui("play").unwrap();
    let rect = h.record_of(h.ui_id(play).unwrap()).unwrap().rect;
    h.move_to(rect.x + 2.0, rect.y + 2.0);
    h.frame(|_| ());
    assert!(h.ui_cache_stats().rebuilt.contains(&play));
    h.move_to(1000.0, 1000.0);
    h.frame(|_| ());
    assert!(h.ui_cache_stats().rebuilt.contains(&play), "it draws its exit");
    h.frame(|_| ());
    assert!(!h.ui_cache_stats().rebuilt.contains(&play));
  }

  #[test]
  fn an_easing_scroll_area_rebuilds_itself_and_replays_its_rows() {
    let mut h = UiHarness::new(640.0, 480.0);
    let rows = (0..30).map(|i| UiNode::button(format!("Row {i}")).name(format!("r{i}")));
    let area = h.spawn_ui(UiNode::scroll().height(100.0).width(200.0).name("area").children(rows));
    warm(&mut h);
    let rect = h.record_of(h.ui_id(area).unwrap()).unwrap().rect;
    h.move_to(rect.x + 150.0, rect.y + 50.0); // over the area, away from row 0
    h.wheel(0.0, -3.0); // starts an ease
    h.frame(|_| ());
    h.move_to(1000.0, 1000.0);
    h.frame(|_| ()); // still easing; nothing hovered
    let stats = h.ui_cache_stats();
    assert!(stats.rebuilt.contains(&area), "the area draws its moving bar");
    let r0 = h.find_ui("r0").unwrap();
    assert!(!stats.rebuilt.contains(&r0), "its rows replay: the shift is applied at resolve");
  }

  #[test]
  fn caching_off_builds_everything() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(menu());
    h.set_ui_cache(false);
    warm(&mut h);
    h.frame(|_| ());
    assert_eq!(h.ui_cache_stats().replayed, 0);
  }

  #[test]
  fn a_bound_sibling_written_during_the_walk_shows_the_value_that_frame() {
    let tree = || UiNode::column().focus_scope()
      .child(UiNode::slider(0.0..=1.0, 0.0).bind("volume").name("s1").default_focus())
      .child(UiNode::slider(0.0..=1.0, 0.0).bind("volume").name("s2"));
    let run = |cache: bool| {
      let mut h = UiHarness::new(640.0, 480.0);
      h.set_ui_cache(cache);
      h.spawn_ui(tree());
      warm(&mut h);
      h.action("ui.right"); // the focused slider steps
      h.frame(|_| ());
      let s2 = h.find_ui("s2").unwrap();
      (h.world().get::<UiValue>(s2).map(|v| v.0), h.commands_debug())
    };
    assert_eq!(run(true), run(false));
  }
}
```


- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement** by the rules.
- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes; every existing node test still passes with caching on (the default).
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/nodes/draw.rs src/core/ui/nodes/bindings.rs src/core/ui/nodes/cache.rs src/core/testing/ui.rs src/core/engine/frame.rs
git commit -m "feat: the node walk replays clean subtrees instead of building them

- A changed node rebuilds with its ancestors; its clean siblings replay
- Writes made during the walk mark what they write; a switch builds everything"
```

---

### Task 7: Embedded trees and list rows

**Files:**
- Modify: `src/core/ui/nodes/draw.rs` (`UiContext::node(root, world)`: refresh and replay or build, no taint), `src/core/ui/nodes/list.rs` (`Pool::contents`: a row replays or builds; a row given another index or rebound rebuilds), `src/core/ui/tape.rs` (remove the embedded-tree taint)
- Test: `src/core/ui/nodes/list.rs` and `src/core/ui/nodes/draw.rs` (`mod cache_tests`)

**Interfaces:**
- Consumes: Task 6's walk.

Rules:
- **Embedded:** `ui.node(root, world)` syncs the root, refreshes the dirty set (a write the same `ui` made before the call is seen), marks live roles, then `draw_node(root)` replays or builds it into the closure's open container. The closure's own tape (none: closure UI is never taped) is unaffected; an embedded tree inside a node tree's... (not possible: embedded roots are drawn from closures only).
- **Rows:** `Pool::contents` calls `sync_root(row)`; if it wrote, refresh the dirty set; then `draw_child(row)` replays or builds. A row's ids follow its item index, so a record stores the index it was built at, and a row whose `UiListRow.index` changed is not clean (the tracked component already marks it; check it explicitly too). Replayed rows' ids must reach `FocusWatch` (`scopes.made`): Task 3's `replay_id` inserts them into `used`.
- The `List` node itself is live (Task 5) and always builds; only rows replay.

- [ ] **Step 1: Write the failing tests:**

```rust
// in src/core/ui/nodes/list.rs tests
#[test]
fn unchanged_rows_replay_and_a_rebound_row_rebuilds() {
  let mut h = UiHarness::new(640.0, 480.0);
  let list = h.spawn_ui(UiNode::list(100, 20.0).height(200.0).width(200.0).row_shape(|| UiNode::text("{name}").bound()).name("l"));
  let bind_all = |h: &mut UiHarness| {
    let binds: Vec<UiListBind> = h.messages::<UiListBind>().collect();
    for b in binds { h.ui_set(b.row, "name", format!("Item {}", b.index)); }
  };
  for _ in 0..4 { h.frame(|_| ()); bind_all(&mut h); }
  h.frame(|_| ());
  let stats = h.ui_cache_stats();
  assert!(stats.replayed > 0, "rows replay");
  assert!(stats.rebuilt.contains(&list), "the List node itself always builds");
  let (_, row) = h.world().get::<UiList>(list).unwrap().rows[0];
  h.ui_set(row, "name", "Changed");
  h.frame(|_| ());
  assert!(h.ui_cache_stats().rebuilt.contains(&row));
}

// in src/core/ui/nodes/draw.rs cache_tests
#[test]
fn an_embedded_tree_replays_and_sees_a_write_made_before_it_is_drawn() {
  let mut h = UiHarness::new(640.0, 480.0);
  let root = h.spawn_ui(UiNode::column().embedded().child(UiNode::text("Hp").name("hp")));
  let hp = h.find_ui("hp").unwrap();
  for _ in 0..3 { h.frame_with_world(|ui, world| { ui.column().show(|ui| ui.node(root, world)); }); }
  h.frame_with_world(|ui, world| { ui.column().show(|ui| ui.node(root, world)); });
  assert!(h.ui_cache_stats().replayed > 0);
  h.frame_with_world(|ui, world| {
    world.get_mut::<UiLabel>(hp).unwrap().0 = "Hp 9".into();
    ui.column().show(|ui| ui.node(root, world));
  });
  assert!(h.ui_cache_stats().rebuilt.contains(&hp));
}
```

(If `ui.node` returns a value, ignore it; adjust to its signature.)

- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement** by the rules.
- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes (the list focus tests from 4e are the guard for `FocusWatch`).
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/nodes/draw.rs src/core/ui/nodes/list.rs src/core/ui/tape.rs
git commit -m "feat: embedded node trees and list rows replay when unchanged

- An embedded tree sees writes made before it is drawn that frame
- A rebound or re-indexed row rebuilds; the List node always builds"
```

---

### Task 8: The differential oracle

**Files:**
- Create: `src/core/testing/fixtures.rs` (`settings_tree()`, the 200-widget settings node tree the bench uses, moved here and `#[doc(hidden)] pub`), `src/core/ui/nodes/oracle_tests.rs` (`#[cfg(test)]`)
- Modify: `src/core/testing/mod.rs`, `src/core/ui/nodes/mod.rs` (the test module), `src/core/testing/ui.rs` (`frame_snapshot()`), `benches/ui.rs` (use `ryo2d::testing::fixtures::settings_tree`)
- Test: `src/core/ui/nodes/oracle_tests.rs`

**Interfaces:**
- Produces:
  ```rust
  impl UiHarness {
    /// Everything the oracle compares about the last frame: draw commands, every record, focus, the events and every UI component's value, in Debug form
    #[doc(hidden)] pub fn frame_snapshot(&self) -> String;
  }
  // src/core/ui/nodes/oracle_tests.rs
  fn oracle(setup: impl Fn(&mut UiHarness), steps: &[Step]); // two harnesses, caching on and off, same setup and steps; asserts equal snapshots after every frame
  enum Step { Frame, Move(f32, f32), Press, Release, Wheel(f32), Action(&'static str), Set(&'static str, &'static str, BindValue), Write(&'static str, fn(&mut World, Entity)), Theme, Resize(f32, f32), Count(&'static str, usize) }
  ```

Rules:
- `frame_snapshot` includes: the frame's `UiCommands` (`Debug`), every record in `UiMemory`'s drawn records sorted by id (`Debug`), `focused()`, the frame's `UiEvent`s, and every UI node entity's tracked components (sorted by entity index) in `Debug`. Two runs that differ in anything a player or the game can see differ here.
- The oracle runs each scenario on two fresh harnesses (`set_ui_cache(true)` / `(false)`) and, after every step that runs a frame, asserts the snapshots are equal, naming the step index on failure.
- Scenarios (each a `#[test]`):
  1. the settings tree, idle, 30 frames;
  2. the settings tree, pad walking: 40 × `ui.down`, then `ui.up` × 10, Back;
  3. the settings tree, pointer: moves across rows, a press and release on a toggle, a drag of a slider, the wheel over the scroll area, a scrollbar drag;
  4. bindings: `ui_set` of a name two widgets show, `update_ui` with a changed label, a component written by game code between frames;
  5. a modal opening over the settings tree (spawn `UiNode::confirm(..)`), walking it, closing it;
  6. a text field: focus it, type (use the harness's text-input path; add `UiHarness::type_text(&str)` if missing), submit;
  7. a node list: 1,000 rows bound by name, scroll, jump (`ui_list_scroll_to`), count change, refresh;
  8. environment: a theme switch, a resize, a pad-family change (set the last device to a pad);
  9. drag and drop: a grid of `UiNode::slot()` drag sources and drop targets (an inventory); pick an item up, move over several targets and between them, drop on one, cancel another drag with Back;
  10. a seeded random run: 3,000 frames of random steps (pointer moves within the screen, presses, releases, wheel, `ui.up/down/left/right`, confirm, Back, occasional `ui_set` and theme switch), from a fixed seed (a small xorshift in the test, no new dependency).

- [ ] **Step 1: Write the oracle and the scenarios** (the helper first, then each scenario). Example of the shape:

```rust
#[test]
fn the_settings_tree_walked_by_pad_looks_the_same_cached() {
  oracle(
    |h| { h.spawn_ui(crate::core::testing::fixtures::settings_tree()); },
    &[Step::Frame, Step::Frame]
      .into_iter()
      .chain(std::iter::repeat([Step::Action("ui.down"), Step::Frame]).take(40).flatten())
      .chain([Step::Action("ui.back"), Step::Frame])
      .collect::<Vec<_>>(),
  );
}
```

- [ ] **Step 2: Run them** — `cargo test -p ryo2d --lib core::ui::nodes::oracle_tests`. Every scenario must pass. A failure is a caching bug from Tasks 3-7: find the cause (systematic debugging; the snapshot diff names the first differing field), fix it in the owning code with a focused test, and list each fix in the report. Do not weaken a scenario.
- [ ] **Step 3: Run the gate and commit**:

```bash
git add src/core/testing/fixtures.rs src/core/testing/mod.rs src/core/ui/nodes/oracle_tests.rs src/core/ui/nodes/mod.rs src/core/testing/ui.rs benches/ui.rs <files fixed>
git commit -m "test: caching on and off draw and behave the same, frame by frame

- A differential oracle over idle, pad, pointer, bindings, modal, text, list and environment scenarios
- A seeded random-input run of 3,000 frames"
```

---

### Task 9: The oracle over the demos, and the tour

**Files:**
- Create: `examples/demos/ui/tests/cache.rs`, `examples/games/breakout/tests/cache.rs`
- Modify: `src/core/testing/` (`HeadlessEngine`: a per-frame UI snapshot, `#[doc(hidden)]`), `src/core/config/debug.rs` (`DebugConfig.ui_cache: bool`, default true), `src/core/engine/frame.rs` (copy `DebugConfig.ui_cache` into `UiCache.enabled` each pass, when `UiNodes` exists), the `EngineBuilder` file (`EngineBuilder::ui_cache(bool)`, setting that field)
- Test: those two files

Rules:
- For every node page of the `ui` demo (menu, settings, inventory, dialogue, Mixed, Big list) and breakout's menus and HUD: two `HeadlessEngine`s (caching on and off) driven by the same `RYO_INPUT`-style script (open the page; pad walks; confirm; Back; pointer moves; on Big list: a jump), comparing `ui_snapshot()` every frame.
- The tour: `./dev.sh demos:tour`, every page compared with `/private/tmp/claude-501/4f-tour-before/` (AE 0, bar Task 1's excluded pages). A non-zero page is a bug: bisect the 4f commits in a scratch worktree, find the cause, fix it with a test.

- [ ] **Step 1: Write the demo and breakout oracle tests**, one test per page, on this shape (4e's `examples/demos/ui/tests/acceptance.rs` has the `pad` helper and the `Demo` setup; reuse them, and the page types `examples/demos/ui/src/lib.rs` registers):

```rust
// examples/demos/ui/tests/cache.rs
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// One engine with caching on, one with it off, showing `page`
fn both(page: fn() -> Box<dyn DemoPage>) -> Option<(HeadlessEngine, HeadlessEngine)> {
  let make = |cache: bool| {
    let demo = Demo::new("cache").resolution(1024, 768).page(page);
    HeadlessEngine::new_or_skip(demo.engine_builder().ui_cache(cache))
  };
  Some((make(true)?, make(false)?))
}

enum Input { Pad(GamepadButton), Key(KeyCode), Move(f32, f32), Click, Wait(u32) }

/// Drive both engines the same way; their UI must match after every frame
fn same_under(on: &mut HeadlessEngine, off: &mut HeadlessEngine, script: &[Input]) {
  on.step_frames(6).unwrap();
  off.step_frames(6).unwrap();
  for (i, input) in script.iter().enumerate() {
    for engine in [&mut *on, &mut *off] {
      match input {
        Input::Pad(b) => pad(engine, *b),
        Input::Key(k) => engine.tap_key(*k),
        Input::Move(x, y) => engine.move_mouse(*x, *y),
        Input::Click => engine.click(),
        Input::Wait(n) => engine.step_frames(*n).unwrap(),
      }
      engine.step_frames(1).unwrap();
    }
    assert_eq!(on.ui_snapshot(), off.ui_snapshot(), "step {i}");
  }
}

#[test]
fn the_file_menu_page_looks_the_same_cached() {
  let Some((mut on, mut off)) = both(|| Box::new(ui::scenes::FileMenuPage::new())) else { return };
  same_under(&mut on, &mut off, &[
    Input::Pad(GamepadButton::DPadDown), Input::Pad(GamepadButton::DPadDown), Input::Pad(GamepadButton::South),
    Input::Pad(GamepadButton::East), Input::Move(512.0, 300.0), Input::Wait(30), Input::Click, Input::Wait(10),
  ]);
}
```

(`ui_snapshot()` is the `HeadlessEngine` mirror of Task 8's `frame_snapshot()`: add it, `#[doc(hidden)]`, and `move_mouse`/`click` if the engine's test API names them differently. Write the same test for settings, inventory, dialogue, Mixed and Big list (Big list's script adds a jump: focus the field, type a rank, Go), and in `examples/games/breakout/tests/cache.rs` for breakout's main menu, its settings menu and the HUD during play (arrows, Enter, Escape, a few seconds of play with `Wait`).)
- [ ] **Step 2: Run them** — `cargo test -p ui --test cache` and `cargo test -p breakout --test cache` (use the crates' real package names). Every page passes; fix and report as in Task 8.
- [ ] **Step 3: Run the tour and compare.** Record every page's AE in the report.
- [ ] **Step 4: Run the gate and commit**:

```bash
git add examples/demos/ui/tests/cache.rs examples/games/breakout/tests/cache.rs <engine files>
git commit -m "test: the ui demo's node pages and breakout look the same cached

- Caching on and off compared frame by frame under scripted input"
```

---

### Task 10: The spot check and the edge cases

**Files:**
- Modify: `src/core/ui/nodes/cache.rs` (the spot check), `src/core/ui/nodes/draw.rs`, `src/core/testing/ui.rs` (`set_ui_spot_check`, `age_change_ticks`), `src/core/testing/` (`HeadlessEngine` cache stats)
- Create: `tests/e2e/ui_cache.rs`
- Test: `src/core/ui/nodes/cache.rs` (`mod edge_tests`), `tests/e2e/ui_cache.rs`

**Interfaces:**
- Produces: `impl UiHarness { pub fn set_ui_spot_check(&mut self, on: bool); }`; `CacheStats.spot_checked: Option<Entity>`.

Rules:
- **Spot check (debug builds only, `cfg(debug_assertions)`):** each frame one clean node with a record, chosen round-robin over the records, is built instead of replayed; its new tape is compared op by op with the old (`Debug` form). With unchanged inputs they are equal; a difference warns once per entity (`warn_once`) with the node's kind, name and the first differing op, and the new tape replaces the old. It is counted in `spot_checked`, not in `built`. It is on in the engine by default in debug builds and off in `UiHarness` until `set_ui_spot_check(true)`.
- **Edge cases (Review Focus 4 and 5 and the spec's list):** each a test with caching on, checked against caching off with Task 8's `oracle` (move `oracle` to a shared test util if needed).

- [ ] **Step 1: Write the failing tests:**

```rust
#[cfg(test)]
mod edge_tests {
  // the spot check
  #[test]
  fn the_spot_check_catches_a_bypassed_write() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.set_ui_spot_check(true);
    h.spawn_ui(UiNode::column().child(UiNode::text("One").name("t")));
    for _ in 0..3 { h.frame(|_| ()); }
    let t = h.find_ui("t").unwrap();
    h.world_mut().get_mut::<UiLabel>(t).unwrap().bypass_change_detection().0 = "Two".into();
    for _ in 0..10 { h.frame(|_| ()); } // one record a frame: it reaches `t`
    assert_eq!(crate::core::ui::test_util::warnings_containing("cached UI node"), 1);
  }

  // despawning inside a cached subtree, reparenting, hiding and disabling: on and off must match
  fn menu() -> UiNode {
    UiNode::column().focus_scope().name("menu")
      .child(UiNode::row().name("pair").child(UiNode::button("A").name("a")).child(UiNode::button("B").name("b")))
      .child(UiNode::column().name("other").child(UiNode::button("C").name("c")))
  }

  #[test]
  fn despawning_inside_a_cached_subtree_looks_the_same() {
    oracle(|h| { h.spawn_ui(menu()); }, &[Step::Frame, Step::Frame, Step::Write("a", |w, e| { w.despawn(e); }), Step::Frame, Step::Frame]);
  }

  #[test]
  fn reparenting_a_cached_node_looks_the_same() {
    oracle(|h| { h.spawn_ui(menu()); }, &[
      Step::Frame, Step::Frame,
      Step::Write("b", |w, e| { let other = crate::core::ui::nodes::find_ui(w, "other").unwrap(); w.entity_mut(e).insert(ChildOf(other)); }),
      Step::Frame, Step::Frame,
    ]);
  }

  #[test]
  fn hiding_and_disabling_a_cached_node_looks_the_same() {
    oracle(|h| { h.spawn_ui(menu()); }, &[
      Step::Frame, Step::Frame,
      Step::Write("a", |w, e| { w.entity_mut(e).insert(UiHidden); }), Step::Frame,
      Step::Write("a", |w, e| { w.entity_mut(e).remove::<UiHidden>(); }), Step::Frame,
      Step::Write("c", |w, e| { w.entity_mut(e).insert(UiDisabled); }), Step::Frame,
      Step::Write("c", |w, e| { w.entity_mut(e).remove::<UiDisabled>(); }), Step::Frame, Step::Frame,
    ]);
  }

  // Review Focus 5: change ticks wrap
  #[test]
  fn a_kept_tick_survives_a_tick_check() {
    let mut h = UiHarness::new(640.0, 480.0);
    h.spawn_ui(UiNode::column().child(UiNode::button("A").name("a")));
    for _ in 0..3 { h.frame(|_| ()); }
    h.age_change_ticks(); // advances the world's change tick past bevy's maximum change age, then runs CheckChangeTicks, cache included
    h.frame(|_| ());
    assert_eq!(h.ui_cache_stats().built, 0, "nothing looks changed after the clamp");
    let a = h.find_ui("a").unwrap();
    h.world_mut().get_mut::<UiLabel>(a).unwrap().0 = "B".into();
    h.frame(|_| ());
    assert!(h.ui_cache_stats().rebuilt.contains(&a), "a real change is still seen");
  }
}
```

(`age_change_ticks` is a `#[doc(hidden)]` harness helper: advance the world's change tick by more than bevy's maximum change age in as few calls as bevy allows (bevy's own change-detection tests show how; it must finish in a few seconds), then run the same `CheckChangeTicks` handling the engine runs, the cache's `check_ticks` included. `warnings_containing` is 4e's test helper; the spot check's warning contains "cached UI node". `oracle` and `Step` are Task 8's: move them to a shared `#[cfg(test)]` module if they are private to `oracle_tests.rs`.)

- [ ] **Step 1b: Write the scene-transition test** (Review Focus 4) as an e2e test, `tests/e2e/ui_cache.rs`, with `HeadlessEngine` and Task 9's `ui_snapshot()`: a menu scene spawns a node tree with an "Open" button (default focus); confirming it pushes an overlay scene with its own node tree and a "Close" button; confirming that pops back. While the overlay is shown, the menu scene's `ui` (drawn first, below) writes the overlay tree's label every frame (a counter): the overlay must show this frame's value (Review Focus 3). Run it with caching on and off under the same input (South, wait, South, wait) and compare `ui_snapshot()` every frame; also assert that the overlay's records are gone a frame after it exits (`HeadlessEngine`'s cache stats, `#[doc(hidden)]`). Use the scene API names the existing e2e UI tests use (`tests/e2e/ui_nodes.rs`).

- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement** by the rules.
- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes.
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/nodes/cache.rs src/core/ui/nodes/draw.rs src/core/testing/ui.rs tests/e2e/ui_cache.rs <HeadlessEngine file>
git commit -m "feat: a debug spot check that a cached UI node still builds the same

- One record a frame is rebuilt and compared; a difference warns once
- Scene transitions, tick wrap, despawn, reparent and hidden toggles tested on and off"
```

---

### Task 11: The debug view and the switch

**Files:**
- Modify: `src/core/ui/debug.rs` (the rebuilds overlay), `src/core/console/builtins.rs` (`debug:ui-rebuilds`, `ui:cache on|off`, help lines), `src/core/config/debug.rs` (`DebugConfig.show_ui_rebuilds: bool`; `ui_cache` exists since Task 9), `src/core/engine/frame.rs` (draw the overlay)
- Test: `src/core/console/builtins.rs` tests and `src/core/ui/debug.rs` tests

Rules:
- `debug:ui-rebuilds` toggles `show_ui_rebuilds`. The overlay, on the overlay layer like `debug:ui`, outlines each node rebuilt this frame (its last-frame record rect), each outline fading out over 0.5 s (the overlay keeps `(UiId, seconds left)`, aged by `dt`), and writes "rebuilt N / replayed M nodes" in a corner.
- `ui:cache off` / `ui:cache on` set `DebugConfig.ui_cache` (Task 9 added it, its per-pass copy into the cache and `EngineBuilder::ui_cache`); `ui:cache` alone prints the state.
- With caching off, the frame is exactly the uncached path (Task 6's switch).

- [ ] **Step 1: Write the failing tests:**

```rust
// src/core/console/builtins.rs tests, beside the debug:ui test
#[test]
fn debug_ui_rebuilds_toggles_the_overlay() {
  let (registry, mut ctx) = test_registry(); // the existing debug:ui test's setup
  assert_eq!(registry.execute("debug:ui-rebuilds", &mut ctx), CommandResult::Ok);
  assert!(ctx.debug_config.show_ui_rebuilds);
  registry.execute("debug:ui-rebuilds", &mut ctx);
  assert!(!ctx.debug_config.show_ui_rebuilds);
}

#[test]
fn ui_cache_turns_caching_off_and_on() {
  let (registry, mut ctx) = test_registry();
  assert!(ctx.debug_config.ui_cache, "on by default");
  assert_eq!(registry.execute("ui:cache off", &mut ctx), CommandResult::Ok);
  assert!(!ctx.debug_config.ui_cache);
  registry.execute("ui:cache on", &mut ctx);
  assert!(ctx.debug_config.ui_cache);
}

// src/core/ui/debug.rs tests
#[test]
fn a_rebuilt_node_is_outlined_and_the_outline_fades() {
  let mut h = UiHarness::new(640.0, 480.0);
  h.spawn_ui(UiNode::column().child(UiNode::button("A").name("a")));
  for _ in 0..3 { h.frame(|_| ()); }
  let a = h.find_ui("a").unwrap();
  h.world_mut().get_mut::<UiLabel>(a).unwrap().0 = "B".into();
  let mut overlay = RebuildsOverlay::default();
  h.frame_with_world(|ui, world| overlay.draw(ui, world, 1.0 / 60.0));
  assert!(overlay.outlined() >= 1, "the rebuilt path is outlined");
  for _ in 0..40 { h.frame_with_world(|ui, world| overlay.draw(ui, world, 1.0 / 60.0)); }
  assert_eq!(overlay.outlined(), 0, "faded after half a second");
}
```

(Use the setup the existing `debug:ui` test uses in place of `test_registry`. `RebuildsOverlay` is the overlay's state: `draw(ui, world, dt)` outlines last frame's rebuilt nodes and ages the rest; `outlined()` counts live outlines.)
- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the tests** and launch nothing interactive.
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/debug.rs src/core/console/builtins.rs src/core/config/debug.rs src/core/engine/frame.rs <builder file>
git commit -m "feat: see which UI nodes rebuild, and turn caching off

- debug:ui-rebuilds outlines rebuilt subtrees, fading, with a counter
- ui:cache on|off and EngineBuilder::ui_cache"
```

---

### Task 12: Benches and the allocation guard

**Files:**
- Modify: `benches/ui.rs`
- Create: `tests/ui_cache_alloc.rs`

Rules:
- New benches: "ui frame, the 200-widget settings screen as nodes, unchanged" (the settings tree, no input, after warm-up; **target under 35 µs**); "ui frame, a 2,000-node static tree" (a grid of 2,000 text and button nodes, unchanged; reported, with the cost per replayed node); and the dirty check alone (a `#[doc(hidden)]` harness hook that runs `refresh_dirty` + `mark_live` on the settings tree; reported).
- The existing node settings bench (focus every 10th frame) is reported; the guard still holds.
- `tests/ui_cache_alloc.rs` (one test in the binary, the counting allocator as in `tests/ui_alloc.rs`): an unchanged frame of the settings node tree with caching on allocates no more than with caching off, and two consecutive unchanged cached frames allocate the same count (replay allocates nothing).
- Check `uptime` before benching; wait for a 1-minute load under 3 (Monitor/until-loop, up to 30 minutes); run twice; report both with load. If the target is missed in a quiet run, profile (xctrace Time Profiler, as in 4e) and fix within Tasks 3-7's code with a test; else report the numbers and the reason.

- [ ] **Step 1: Add the benches and the allocation test.**
- [ ] **Step 2: Run them** (`cargo bench --bench ui`, `cargo test --test ui_cache_alloc`).
- [ ] **Step 3: Run the gate and commit**:

```bash
git add benches/ui.rs tests/ui_cache_alloc.rs <harness hook>
git commit -m "test: benches and an allocation guard for cached node trees

- The unchanged node settings screen, a 2,000-node static tree and the dirty check"
```

---

### Task 13: Documentation

**Files:**
- Modify: `.claude/skills/ryo-ui/SKILL.md` and `src/cli/skills/ryo-ui.md`, `.claude/skills/ryo-api/SKILL.md` and `src/cli/skills/ryo-api.md`, `.claude/skills/ryo-engine-dev/SKILL.md` and `src/cli/skills/ryo-engine-dev.md` (the module map, the console commands if it lists them), `src/core/ui/AGENTS.md`, `docs/architecture/overview.md`
- Test: `cargo test -p ryo2d --lib embedded_skills_are_their_sources`, `cargo test -p ryo2d --doc`

Contents (read the code for final names):
- **ryo-ui:** a "Cached node trees" part: node trees rebuild only what changed (what counts: components, `ui_set`, `update_ui`, interaction, live kinds, theme/scale/fonts/prompts); the bypass rule (`bypass_change_detection` writes are invisible to the UI); `debug:ui-rebuilds` and `ui:cache`; closure UI is unchanged.
- **ryo-api:** `UiHarness::{set_ui_cache, ui_cache_stats, set_ui_spot_check, frame_snapshot}`, `UiCacheStats`, `EngineBuilder::ui_cache`.
- **src/core/ui/AGENTS.md:** the tape (ops, child slots, ids and state logs, taint and why), the record and where it lives (`UiNodes`, not components: archetypes), the dirtiness sources and why each exists, splice-or-build and why dirtiness goes up not across, writes during the walk, the spot check, the oracle as the test of record.
- **overview.md:** one paragraph in the UI section.

- [ ] **Step 1: Write the docs**; after each skill, `cp` and `cmp`.
- [ ] **Step 2: Run the checks.**
- [ ] **Step 3: Run the gate and commit**:

```bash
git add <the files above>
git commit -m "docs: cached node trees

- What rebuilds a node, the bypass rule, the debug view and the switch
- Module notes on the tape, the records, dirtiness and the oracle"
```

---

### Task 14: Measure again

**Files:**
- Modify: `docs/backlog/ui-retained-nodes.md`

Rules:
- Profile the unchanged and the changing node settings frames, the 2,000-node tree and the closure settings frame (xctrace Time Profiler, scratch in `/private/tmp/claude-501/4f-profile-after/`), bucketing as 4e did, plus "replay" and "dirty check" buckets.
- Write a "4f: Cached Node Trees (done)" section: what landed, the before and after numbers, the buckets, and the data for a decision on layout and paint retention (approach B): how much of an unchanged node frame is now layout and resolve, and what B would have to track. Leave `**Recommendation:** _to be written by the controller_`; the controller writes it and the owner decides.
- Move 4f's "Left for later": layout and paint retention (pending the decision), and anything the tasks deferred.

- [ ] **Step 1: Profile.**
- [ ] **Step 2: Write the note** (without the recommendation).
- [ ] **Step 3: Commit** (after the controller adds the recommendation):

```bash
git add docs/backlog/ui-retained-nodes.md
git commit -m "docs: 4f's numbers and whether to retain layout and paint

- Before and after frames; the new profile of cached node frames"
```
