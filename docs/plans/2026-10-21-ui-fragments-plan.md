# UI Fragments (4i) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A node tree's unchanged subtrees keep their layout and resolved output between frames, in the UI pass and in the UI renderer, and scroll areas draw their contents through whole-pixel scroll slots, so a frame costs about what changed.

**Architecture:**
- **Scroll slots (Task 2):** resolve places every node relative to its scroll slot, in render pixels and without any scroll offset. Each scroll area is a slot holding its frame's whole-pixel offset and its clip.
- **Fragments (Tasks 3 to 6):** the node walk pushes each outermost clean subtree that has a fragment as one kept node. The layout engine keeps that node's Taffy subtree without walking it, and resolve copies the fragment's records and commands, re-based to this frame's draw order. A fragment that moved or was resized is resolved again from its kept nodes, against Taffy's current layout.
- **The renderer (Tasks 7 to 9):** fragment runs reach it tagged, so it keeps and copies each run's instances, and the vertex shader moves a slot's instances by the slot's offset.

**Tech Stack:** Rust 2024, bevy_ecs 0.20 (as `ryo2d::ecs`), Taffy, wgpu 30 (WebGPU default limits, no features), WGSL composed with `concat!`, `HeadlessEngine` and `UiHarness` tests, criterion.

**Spec:** `docs/specs/2026-10-21-ui-fragments-design.md`

**Research (read-only, outside the repo):**
- `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/4i-research/excerpts.md`: verbatim excerpts of every function this plan touches, with `path:line`, and an "Observations" section at its end. Read the excerpts your task names before you start; they are faster than searching.
- `profile.md` (where the time goes today), `code-map.md` (the frame's stages and what reuse must check) and `survey.md` (other engines) in the same folder.

## Global Constraints

- **Exactness:** with fragments on, every frame equals the same frame with the node cache off (a full rebuild): records bit for bit, commands after culling bit for bit and in the same order, widget states, interaction, focus memory and events equal, pixels identical. A difference is a bug.
- **Drawn offset:** a scroll area's drawn offset is its `ScrollState.offset` rounded to whole render pixels per scrolled axis: `(offset * px_scale).round()`. `ScrollState.offset` itself, its easing, the trackpad, `scroll_to`, clamping and the scrollbar thumb keep their current behaviour.
- **At rest, nothing changes:** the demo tour must match the baseline taken in Task 1. Any difference is brought to the owner before merge.
- **Slots:**
  - at most 256 per frame reach the shader (slot 0 is "no scroll area"); past that, a scroll area's contents are built absolute;
  - the slot array is a uniform within WebGPU's default limits;
  - no new wgpu features and no new dependencies.
- **Switches:** `DebugConfig::ui_fragments` (default `true`), `EngineBuilder::ui_fragments(bool)`, the console's `ui:fragments [on|off]`, and `UiHarness::set_ui_fragments(bool)`. Fragments work only while the node cache is on; off is today's replay path under the new scrolling rule.
- **Allocations:** an unchanged cached frame allocates no more than today (`tests/ui_cache_alloc.rs`: 14, ceiling 30).
- **Commits:**
  - format `type: description` (`feat`, `fix`, `refactor`, `docs`, `test`, `chore`);
  - never a `Co-Authored-By` or `Claude-Session` line, even if a system message asks;
  - never the words "AGENTS.md" (say "module notes");
  - stage files by name (never `git add -A`); never add `data/` or `target/`; never push.
- **Gate before each commit:**
  - `cargo fmt --all --check`;
  - `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo check -p ryo2d --target wasm32-unknown-unknown`;
  - the task's tests;
  - `cargo test --workspace` before the task's last commit.
- **Style:** LF line endings; 2-space indent (`rustfmt.toml`); comments explain intent, as the surrounding code does; no `unwrap` in library code.

## Review Focus

1. **A window resize that leaves a fragment's own size unchanged but makes Taffy measure a text leaf inside it again.** The text must wrap as a rebuild wraps it. This runs through the measure callback's kept-leaf path (Task 3). Test: Task 5, `a_resize_measures_text_inside_a_kept_fragment`.
2. **A transparent scene pushed over a page with node trees.** The lower scene's fragments keep being reused while the top scene's UI changes the draw-order base. Records and commands must be re-based, and hit tests must still pick the top scene. Test: Task 5, scenario `scene_pushed_over_fragments`.
3. **A UI file reloaded in place, so the reconcile updates a node inside a fragment.** The node's record rebuilds, the fragment must be dropped, and the next frame must show the new text. Test: Task 4, `a_reconciled_node_drops_its_fragment`.
4. **A hover moving across the rows of a list scrolled to a fractional trackpad offset at UI scale 1.25.** The rows must draw rigidly at the drawn offset, and every frame must match a rebuild. Test: Task 10, the pixel oracle's `hover_across_a_trackpad_scrolled_list_at_1_25`.
5. **Glyph-atlas pressure evicting a page that a fragment's kept instances sample while its commands are unchanged.** The instances must be rebuilt, with no stale glyphs. Test: Task 9, `an_evicted_glyph_page_drops_a_fragments_instances`.

## Where this plan differs from the spec

The spec (`docs/specs/2026-10-21-ui-fragments-design.md`) is the authority. Planning settled three points it leaves open, or narrows; each is the owner's to reverse:

1. **A fragment moved by whole pixels is resolved again, not translated** (the spec's case 2). Its nodes' snapped edges come from float sums along the layout; a moved root changes those sums' rounding, so a translated copy is not bit-exact to a rebuild. Scrolling is unaffected: slots carry it, and a scrolled fragment does not move within its slot. A fragment whose layout moves in its slot (a sibling grew) is resolved again from what it kept: no replay, no layout walk.
2. **A subtree holding a node placed with `at` is never a fragment** (beside popups and live nodes). Such a node does not scroll with its area (it lives in slot 0), so its subtree would span two slots.
3. **A rerun UI pass reuses only fragments carried over from the last frame** (the spec says never those of a discarded pass): fragments captured or resolved again in the discarded pass are dropped; those it merely copied go back to the store.

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `src/core/ui/immediate/commands.rs` | modify | `UiDraw` (order, slot, command), `ScrollSlot`, the frame's slot table; `FragmentRun` (Task 7); the culled view for snapshots (Task 6) |
| `src/core/ui/tree.rs` | modify | resolve in render pixels relative to slots (Task 2); kept nodes, capture and emit (Task 4); re-resolve from kept nodes (Task 5) |
| `src/core/ui/fragment.rs` | create | `Fragment`, its key, capture and emit helpers |
| `src/core/ui/layout/engine.rs` | modify | kept nodes: children left as they are, descendants counted, kept alive by the sweep, measured by id; `placed_below` (Tasks 3 and 5) |
| `src/core/ui/memory.rs` | modify | the fragment store (`fragments_prev`/`fragments_next`), versions, the capture log, interaction with `begin_frame`/`restart_pass`/`rewind_frame`/`forget_scene` |
| `src/core/ui/immediate/context.rs` | modify | `push_kept`, order reservation, the capture log while replaying |
| `src/core/ui/tape.rs` | modify | the capture log fed by `replay` (ids, states, popups seen) |
| `src/core/ui/nodes/draw.rs`, `nodes/cache.rs` | modify | the walk pushes kept fragments; stats (`reused`, `resolved_again`, `captured`); `set_fragments_enabled` |
| `src/core/ui/widgets/scroll.rs` | modify | `drawn_px` (the whole-pixel drawn offset) |
| `src/core/ui/debug.rs` | modify | overlay counts (Task 6) |
| `src/core/config/debug.rs`, `src/core/engine/mod.rs`, `src/core/engine/frame.rs`, `src/core/console/builtins.rs` | modify | the switch; the UI pass time |
| `src/core/testing/ui.rs`, `src/core/testing/headless.rs`, `src/core/testing/retain.rs` | modify | `set_ui_fragments`, fragment stats, `ui_pass_time`, `PixelPair::fragments` |
| `src/core/ui/nodes/oracle.rs`, `oracle_tests.rs` | modify | `Fragments(bool)` step; the per-input scenarios (Task 5) |
| `src/core/render/ui_retain/mod.rs` | modify | `keep` per fragment run and slot (Task 7) |
| `src/core/render/ui_retain/fragments.rs` | create | the per-run instance cache (Task 9) |
| `src/core/render/ui_renderer/{build,mod,gpu,instance}.rs`, `src/core/shaders/core/ui.wgsl` | modify | slot-relative commands (Task 2, CPU), the slot uniform and vertex culling (Task 8), run copies (Task 9) |
| `benches/ui.rs`, `benches/ui_render.rs`, `tests/ui_cache_alloc.rs` | modify | the focus bench, the pass guard, the targets |
| `tests/e2e/ui_fragments.rs`, `examples/demos/ui/tests/fragments.rs`, `examples/games/breakout/tests/fragments.rs` | create | the pixel oracles (Task 10) |
| docs: `src/core/ui/AGENTS.md`, `src/core/render/AGENTS.md`, `.claude/skills/ryo-ui/SKILL.md` + `src/cli/skills/ryo-ui.md`, `.claude/skills/ryo-renderer/SKILL.md` + `src/cli/skills/ryo-renderer.md`, `docs/backlog/ui-retained-nodes.md` | modify | Task 11 |

---

### Task 1: The baseline and the UI pass's time

Before anything changes: the demo tour baseline every later task compares against, a reading of the UI pass's own time in the engine, and the bench that moves the focus every frame (today's focus bench moves it every tenth frame).

**Files:**
- Modify: `src/core/engine/mod.rs` (an `ui_pass_time: Duration` field, its accessor), `src/core/engine/frame.rs` (`draw_ui` times the pass)
- Modify: `src/core/testing/headless.rs` (`ui_pass_time`)
- Modify: `benches/ui.rs` (the focus-every-frame bench), `benches/ui_render.rs` (prints the pass time beside each engine's render step)
- Test: `src/core/engine/tests.rs`

**Interfaces:**
- Produces: `HeadlessEngine::ui_pass_time(&self) -> Duration` (`#[doc(hidden)]`): the last frame's UI work in `Engine::draw_ui`, from `ui_commands.clear()` to the end of the overlays and console. `Engine::ui_pass_time(&self) -> Duration` (crate). The bench `"ui frame, the 200-widget settings screen as nodes, focus moving every frame"`.

- [ ] **Step 1: Capture the tour baseline.** From the worktree root, before any code change:

```bash
./dev demos:tour
rm -rf /private/tmp/claude-501/4i-tour/base && mkdir -p /private/tmp/claude-501/4i-tour
cp -R target/demo-tour /private/tmp/claude-501/4i-tour/base
./dev demos:tour
diff -rq target/demo-tour /private/tmp/claude-501/4i-tour/base | grep '\.png' > /private/tmp/claude-501/4i-tour/nondeterministic.txt; cat /private/tmp/claude-501/4i-tour/nondeterministic.txt
```

Expected: the second run differs from the first only on pages known to be nondeterministic (particles, steering: 4h saw 9 such pages). Record the list in your report; later tasks compare every other page byte for byte.

- [ ] **Step 2: Write the failing test** (in `src/core/engine/tests.rs`, beside the other headless engine tests; use the `settings_tree` fixture):

```rust
#[test]
fn the_ui_pass_time_is_measured_and_is_zero_without_ui() {
  let Some(mut empty) = HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(320, 240)) else {
    return;
  };
  empty.step_frames(3).unwrap();
  let empty_time = empty.ui_pass_time();

  let builder = EngineBuilder::new().resolution(1280, 720).start_scene(SpawnUi(settings_tree()));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(5).unwrap();
  assert!(engine.ui_pass_time() > Duration::ZERO, "a UI pass takes time");
  assert!(
    engine.ui_pass_time() > empty_time,
    "a 200-widget screen takes longer than no UI ({:?} against {:?})",
    engine.ui_pass_time(),
    empty_time
  );
}
```

`SpawnUi` is the small test scene the engine tests already use to spawn a tree in `on_enter`, if one exists (search `tests.rs` for `spawn_ui`); otherwise add it there:

```rust
/// Spawns `tree` when it enters
struct SpawnUi(UiNode);

impl Scene for SpawnUi {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(self.0.clone());
  }
}
```

- [ ] **Step 3: Run it to see it fail.**

Run: `cargo test -p ryo2d --lib engine::tests::the_ui_pass_time_is_measured -- --nocapture`
Expected: compile error, `no method named ui_pass_time`.

- [ ] **Step 4: Measure the pass.** In `Engine::draw_ui` (`engine/frame.rs`, excerpt `src/core/engine/frame.rs:564-739`), take `let started = Instant::now();` before `self.ui_commands.clear()` and store `self.ui_pass_time = started.elapsed();` after the console is drawn, before the function returns, on every return path (a rerun pass adds to it: use `self.ui_pass_time += started.elapsed()` on a rerun, `=` on the first pass, as `draw_ui` knows which it is). Add the field (`ui_pass_time: Duration`, `Duration::ZERO` in `Engine::new`) beside `render_time` and the accessors:

```rust
  /// The last frame's UI pass: recording the scenes' UI, the node walk,
  /// layout and resolve, the overlays and the console (not the render side)
  pub(crate) fn ui_pass_time(&self) -> std::time::Duration {
    self.ui_pass_time
  }
```

```rust
  /// The last frame's UI pass on the CPU: the scenes' UI, the node walk,
  /// layout, resolve and the overlays (not the render side)
  #[doc(hidden)]
  pub fn ui_pass_time(&self) -> Duration {
    self.engine.ui_pass_time()
  }
```

- [ ] **Step 5: Run it to see it pass.** Same command. Expected: PASS.

- [ ] **Step 6: The benches.** In `benches/ui.rs`, after `settings_nodes_unchanged`, add (and register it in `criterion_group!`):

```rust
/// The settings tree with `ui.down` pressed every frame: the focus moves
/// down the list each frame, wrapping, so every frame rebuilds the rows it
/// leaves and enters
fn settings_nodes_focus_every_frame(c: &mut Criterion) {
  let mut harness = UiHarness::new(640.0, 480.0);
  harness.spawn_ui(settings_tree());
  for _ in 0..3 {
    harness.frame(|_| {});
  }
  assert!(harness.focused().is_some(), "the screen's scope took focus");
  c.bench_function(
    "ui frame, the 200-widget settings screen as nodes, focus moving every frame",
    |b| {
      b.iter(|| {
        harness.action("ui.down");
        harness.frame(|_| {});
      })
    },
  );
}
```

In `benches/ui_render.rs`'s guard, print each engine's median `ui_pass_time()` beside its render step (median over the same rounds; no assertion yet; Task 10 adds the targets). Run `cargo bench --bench ui -- 'settings screen as nodes'` and `cargo bench --bench ui_render` and put the medians in your report: they are the "before" column of Task 10's table.

- [ ] **Step 7: Commit.**

```bash
git add src/core/engine/mod.rs src/core/engine/frame.rs src/core/engine/tests.rs src/core/testing/headless.rs benches/ui.rs benches/ui_render.rs
git commit -m "test: measure the UI pass's time and bench a focus moving every frame"
```

---

### Task 2: Whole-pixel scrolling and scroll slots

Resolve places every node relative to its scroll slot, in render pixels, without any scroll offset; a scroll area becomes a slot whose whole-pixel offset and clip the frame's commands carry. Records stay absolute. The renderer applies a slot's offset and clip on the CPU here (Task 8 moves that into the shader).

**Files:**
- Modify: `src/core/ui/immediate/commands.rs` (`UiDraw`, `ScrollSlot`, the slot table, `UiCommand::moved`)
- Modify: `src/core/ui/tree.rs` (resolve's placement, records, draws; `Placement` gains `cull`; `command_of` and `nine_slice` return each command with the box it is culled by)
- Modify: `src/core/ui/widgets/scroll.rs` (`drawn_px`)
- Modify: `src/core/render/ui_renderer/build.rs` (a slot's offset and clip applied before building)
- Modify: `src/core/render/ui_retain/mod.rs` (`keep` compares each draw's slot and the slot table)
- Modify: every reader of `UiCommands::as_slice()` / `iter()` (search them: the UI tests, the console and overlay tests, `testing/ui.rs`)
- Test: `src/core/ui/tree.rs` tests (or `src/core/ui/tests/scroll.rs` where the scroll tests live), `src/core/render/ui_retain/tests.rs`, `src/core/render/ui_renderer/tests.rs`

Read first: excerpts `src/core/ui/tree.rs:288-501` (resolve), `:675-895` (`command_of`), `:897-933` (`text_command`), `src/core/render/ui.rs:136-145` (`snap_rect`), `src/core/ui/widgets/scroll.rs:459-579` (`Scroll::show`), `src/core/render/ui_retain/mod.rs:1-146` (`keep`), `src/core/render/ui_renderer/build.rs:185-317`.

**Interfaces:**
- Produces (in `ui/immediate/commands.rs`):

```rust
/// A scroll area's slot in a frame: what the area shows is placed relative
/// to it, so moving the slot moves its contents exactly
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollSlot {
  /// Where the contents' origin is drawn, in whole render pixels
  pub(crate) offset: [i32; 2],
  /// The clip the contents draw under (UI units, absolute)
  pub(crate) clip: Option<Rect>,
}

/// One UI draw: its place in the painter's order, the slot its positions
/// and clip are relative to (0: none, positions absolute), and the command
#[derive(Debug, Clone)]
pub(crate) struct UiDraw {
  pub(crate) order: u32,
  pub(crate) slot: u16,
  pub(crate) command: UiCommand,
}
```

  `UiCommands` holds `draws: Vec<UiDraw>` and `slots: Vec<ScrollSlot>` (slot 0 is pushed by `clear()`: `ScrollSlot { offset: [0, 0], clip: None }`), with `push(order, slot, command)`, `push_slot(slot: ScrollSlot) -> u16` (returns `u16::MAX` past `u16::MAX - 1` slots, which callers treat as "no slot": place absolute), `as_slice(&self) -> &[UiDraw]`, `slots(&self) -> &[ScrollSlot]`, and, on `UiCommand`, `moved(&self, by: Vec2) -> UiCommand` (rect, sprite rect, line `at`, text `origin`, and the command's own clip, each moved by `by`).
- Produces (in `widgets/scroll.rs`): `pub(crate) fn drawn_px(offset: Vec2, axes: [bool; 2], px_scale: f32) -> [i32; 2]`: `(offset[axis] * px_scale).round()` on each scrolled axis, 0 on the other, 0 when not finite.

**The rule, in resolve** (replaces the placement loop at `tree.rs:365-412`; everything in render pixels is computed in `f32`):

- `layout_shift[i]`: today's `shift[i]` without the scroll offsets (the Stack offset and the `at` correction stay).
- `exact[i] = origin + placed[i].rect.xy + layout_shift[i]` (UI units, as today but unscrolled).
- `node_slot[i]`: 0 for the root and for a node with an `at` rect (it goes where it is put, as today); otherwise `content_slot[parent]`.
- `content_slot[i]`: a new slot when node `i` scrolls (`style.scroll_x || style.scroll_y`) and has a `ScrollState`; otherwise `node_slot[i]`.
- Per slot (in a resolve-local table; slot 0 has `base = [0, 0]`, `offset = [0, 0]`): `base` is the slot's origin unscrolled, `offset` where it is drawn, both whole render pixels.
- A node's edges, relative to its slot's `base`: `x0 = (exact.x * s - base.x).round()`, `x1 = (exact.right() * s - base.x).round()`, the same on y. Its rect relative to its slot is `Rect::new(x0 / s, y0 / s, x1 / s - x0 / s, y1 / s - y0 / s)`. Its absolute rect is the same with `x0 + offset.x` and `x1 + offset.x` (and y). For slot 0 both equal today's `snap_rect(exact, s)` bit for bit, as `v - 0.0 == v`.
- A scroll node `i` with `k = content_slot[i]`: `base[k] = base[node_slot[i]] + [x0, y0]` of `i`; `offset[k] = offset[node_slot[i]] + [x0, y0] - drawn_px(state.offset, axes, s)`; its contents' clip (absolute) is today's: `intersect(clip_abs[i], rect_abs[i])` (or `rect_abs[i]` with no clip). Push it: `k = cx.commands.push_slot(ScrollSlot { offset, clip })`.
- `clip_abs[i]`: as today's `clips[i]`, computed from absolute rects.
- Records: `rect: rect_abs[i]`, `clip: clip_abs[i]`, the rest as today.
- Draws: `Placement { rect: rect_rel[i], exact: Rect::new(rect_rel[i].x, rect_rel[i].y, exact[i].width, exact[i].height), clip, cull, layer }`.
  - In slot 0: `clip` and `cull` are both `clip_abs[i]`.
  - In a slot `k > 0`: `clip` is `None` (the slot's clip applies) and `cull` is the slot's clip moved into the slot's coordinates, `clip.map(|c| moved(c, -Vec2::new(offset.x as f32, offset.y as f32) / s))`.
- `command_of` returns `Option<(UiCommand, Option<Rect>)>`, the command and the box it is culled by (`None`: never culled, as a line today); the caller pushes it unless `outside(place.cull, box)`. `nine_slice` does the same for each piece. The commands' `clip` field is `place.clip`.
- `ScrollState.view` and `content` are written from the relative rect's size and `exact`, as today.

**In the renderer** (`build_command`, before anything else): for a draw in a slot `k > 0`, `let slot = slots[k]; let by = Vec2::new(slot.offset[0] as f32, slot.offset[1] as f32) / space.px_scale; let command = draw.command.moved(by);` and its clip becomes `slot.clip` intersected with the moved command's own clip (`None` and `None` is `None`). Then build exactly as today. A draw whose slot index is past the table is skipped.

**In `UiRetain::keep`**: the frame is kept only if every draw's `order`, `slot` and command match as today and the slot table equals last frame's (`PartialEq`); store the slot table with the drawn commands.

- [ ] **Step 1: Write the failing tests.** In `src/core/ui/widgets/scroll.rs`'s `tests` module:

```rust
/// A list of 30 rows of 20 units in a 100-unit-tall scroll area at (0, 0)
fn scrolled_list(ui: &mut UiContext<'_>) {
  ui.scroll().height(100.0).width(200.0).show(|ui| {
    for i in 0..30 {
      ui.button(&format!("Row {i}")).width(200.0).height(20.0).show();
    }
  });
}

#[test]
fn a_scroll_area_draws_its_rows_in_a_slot_moved_by_whole_pixels() {
  let mut h = UiHarness::new(640.0, 480.0);
  h.set_ui_scale(1.25); // whole render pixels are 0.8 units
  h.frame(scrolled_list);
  h.move_to(50.0, 50.0);
  h.trackpad(0.0, -13.3); // a fractional offset, at once
  h.frame(scrolled_list);
  let commands = h.commands();
  let slots = commands.slots();
  assert_eq!(slots.len(), 2, "slot 0 and the scroll area's");
  let rows: Vec<&UiDraw> = commands
    .as_slice()
    .iter()
    .filter(|d| d.slot == 1 && matches!(d.command, UiCommand::Rect { .. }))
    .collect();
  assert!(!rows.is_empty(), "the rows draw in the scroll area's slot");
  // The rows are where an unscrolled list puts them, relative to the slot
  let unscrolled_first = rows[0].command.clone();
  h.trackpad(0.0, -7.9);
  h.frame(scrolled_list);
  let moved = h.commands();
  let first = moved
    .as_slice()
    .iter()
    .find(|d| d.slot == 1 && matches!(d.command, UiCommand::Rect { .. }))
    .unwrap();
  assert_eq!(
    format!("{:?}", first.command),
    format!("{unscrolled_first:?}"),
    "scrolling moves the slot, not its contents"
  );
  let (a, b) = (slots[1].offset, moved.slots()[1].offset);
  assert_ne!(a, b, "the slot moved");
  // The slot moved by the change in the drawn offset: the scroll offset
  // rounded to whole render pixels (read with `ui.scroll_offset(id)` in
  // each frame; `first_offset` and `second_offset` are those readings)
  let drawn = |offset: f32| (offset * 1.25).round() as i32;
  assert_eq!(
    b[1] - a[1],
    -(drawn(second_offset.y) - drawn(first_offset.y)),
    "the drawn offset is the scroll offset rounded to whole render pixels"
  );
}

#[test]
fn records_under_a_slot_are_absolute() {
  let mut h = UiHarness::new(640.0, 480.0);
  h.frame(scrolled_list);
  h.move_to(50.0, 50.0);
  h.trackpad(0.0, -40.0);
  h.frame(scrolled_list);
  // Row 2 started at y = 40: scrolled up by 40 it is at the area's top
  let row = h.record_named("Row 2").expect("row 2 has a record");
  assert_eq!(row.rect.y, 0.0, "a record is where the row is drawn");
}

#[test]
fn a_page_at_rest_draws_as_it_did() {
  // No scroll offset: slot 0 places everything exactly as snap_rect did
  let mut h = UiHarness::new(640.0, 480.0);
  h.set_ui_scale(1.25);
  h.frame(|ui| {
    ui.column().at(Rect::new(10.3, 7.7, 300.0, 200.0)).gap(3.3).show(|ui| {
      ui.button("A").show();
      ui.text("Some text that wraps at the column's width").wrap(true).show();
    });
  });
  for d in h.commands().as_slice() {
    assert_eq!(d.slot, 0);
    if let UiCommand::Rect { rect, .. } = &d.command {
      assert_eq!(*rect, snap_rect(*rect, 1.25), "rects stay on the pixel grid");
    }
  }
}
```

`scrolled_list` returns the scroll area's `Response::id`; read `first_offset` and `second_offset` with `ui.scroll_offset(id)` inside the frames after each trackpad step (the frame closure captures them). The helpers `h.commands()` and `h.record_named(..)` may not exist under these names: use what `UiHarness` (`src/core/testing/ui.rs`) offers (it exposes the frame's commands for tests and records by id), or add `#[cfg(test)]` helpers in the test module. Keep the assertions.

In `src/core/render/ui_retain/tests.rs`:

```rust
#[test]
fn a_frame_whose_only_change_is_a_slot_offset_is_not_kept() {
  let mut retain = UiRetain::default();
  let env = test_env();
  let mut commands = UiCommands::default();
  commands.clear();
  let slot = commands.push_slot(ScrollSlot { offset: [0, -10], clip: Some(Rect::new(0.0, 0.0, 100.0, 100.0)) });
  commands.push(0, slot, rect_command(Rect::new(0.0, 0.0, 50.0, 20.0)));
  assert!(!retain.keep(&commands, &env), "the first frame is drawn");
  commands.clear();
  let slot = commands.push_slot(ScrollSlot { offset: [0, -11], clip: Some(Rect::new(0.0, 0.0, 100.0, 100.0)) });
  commands.push(0, slot, rect_command(Rect::new(0.0, 0.0, 50.0, 20.0)));
  assert!(!retain.keep(&commands, &env), "a moved slot moves pixels");
  assert!(retain.keep(&commands, &env), "unchanged, it is kept");
}
```

(`test_env` and `rect_command` stand for the test module's existing helpers; use theirs.)

In `src/core/render/ui_renderer/tests.rs`: a rect at `(0, 0, 50, 20)` in a slot with `offset: [0, 7]` and `clip: Some(Rect::new(0, 5, 100, 10))` builds the same instance (data and clip) as the absolute rect `(0, 7, 50, 20)` with clip `(0, 5, 100, 10)` in slot 0.

- [ ] **Step 2: Run them to see them fail.**

Run: `cargo test -p ryo2d --lib scroll slot keep_ -- --nocapture`
Expected: compile errors (`UiDraw`, `ScrollSlot`, `push_slot`, `slots` do not exist).

- [ ] **Step 3: Implement** the types, `drawn_px`, resolve's placement by the rule above, the `command_of`/`nine_slice` split, the renderer's slot application and `keep`'s comparison. Update every reader of `as_slice()`/`iter()` to `UiDraw`. The snapshot (`testing/ui.rs` `ui_snapshot`) prints the slot table after the commands (`slots {:?}`).

- [ ] **Step 4: Run them to see them pass,** then the whole crate.

Run: `cargo test -p ryo2d --lib` then `cargo test --workspace`
Expected: PASS. Tests that asserted a scrolled widget's rect at a fractional offset (scroll easing tests, `scroll_to` tests, list tests) may now read the whole-pixel position. Change such an assertion only when the new value is the drawn offset rounded to whole render pixels, and list every test you changed, with old and new values, in your report.

- [ ] **Step 5: The tour.** Run `./dev demos:tour` and compare with the baseline:

```bash
diff -rq target/demo-tour /private/tmp/claude-501/4i-tour/base | grep '\.png' | grep -v -F -f /private/tmp/claude-501/4i-tour/nondeterministic.txt
```

Expected: no output. Every page rests at offset 0 or a whole one. Any page listed is a finding: report the page, the pixel count and the largest channel difference (`magick compare -metric AE`), and stop for a ruling.

- [ ] **Step 6: Commit.**

```bash
git add src/core/ui/immediate/commands.rs src/core/ui/tree.rs src/core/ui/widgets/scroll.rs src/core/render/ui_renderer/build.rs src/core/render/ui_retain/mod.rs src/core/testing/ui.rs src/core/ui/widgets/scroll.rs src/core/render/ui_retain/tests.rs src/core/render/ui_renderer/tests.rs  # and, by name, any other test file Step 4 changed
git commit -m "feat: scroll areas draw their contents through whole-pixel slots"
```

---

### Task 3: The layout engine keeps a subtree without walking it

A node described as kept keeps its Taffy children as they are; its descendants count as laid out and survive the end-of-frame sweep; a text leaf inside it is measured by its id when Taffy lays it out again; and `placed_below` reads a kept subtree's layout.

**Files:**
- Modify: `src/core/ui/layout/engine.rs` (and its `tests` module)
- Modify: `src/core/ui/tree.rs` (the measure closure takes the new target; kept descs pass `kept: true`, set by Task 4)

Read first: excerpts `src/core/ui/layout/engine.rs:26-58`, `:82-93`, `:122-150`, `:157-377`.

**Interfaces:**
- Produces:

```rust
/// Which node Taffy asks to be measured
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Measured {
  /// The node at this index of the descriptions laid out
  Index(usize),
  /// A node inside a kept node's subtree, by its id
  Kept(UiId),
}

/// Sizes a text node: which node, the dimensions already known and the space offered
pub(crate) type MeasureFn<'a> =
  dyn FnMut(Measured, (Option<f32>, Option<f32>), (Avail, Avail)) -> Vec2 + 'a;
```

  `NodeDesc` gains `pub(crate) kept: bool`: "laid out in an earlier frame and kept: its children in Taffy are left as they are (its `children` slice is empty), and its descendants count as laid out". `Entry` gains `below: u32` (the entries in its subtree, counted when it was laid out with its children). `LayoutEngine` gains `kept: Vec<NodeId>` (cleared by `begin_frame`) and:

```rust
  /// Whether a node with this id was laid out and is still kept
  pub(crate) fn holds(&self, id: UiId) -> bool;

  /// The layout of a kept subtree as this frame's layout left it: `ids` are
  /// its nodes, root first, parents before children, `parents[i]` the index
  /// of node `i`'s parent in `ids` (`None` for the root), and the root at
  /// `root_rect` (relative to its subtree's root, as `layout` gives it). The
  /// rects accumulate as `layout`'s do. False, with `out` cleared, if a node
  /// is not held.
  pub(crate) fn placed_below(
    &self,
    root_rect: Rect,
    ids: &[UiId],
    parents: &[Option<usize>],
    out: &mut Vec<Placed>,
  ) -> bool;
```

**Rules for `try_layout`:**
- For a kept desc with an entry: refresh `entry.frame` as today, and when it was not yet laid this frame, add `1 + entry.below` to `laid`. Compare the hash and content key as today. Never compare or set its children. Push its `NodeId` onto `self.kept`.
- A kept desc without an entry is a caller's mistake: return a `TaffyError`-free failure path by laying it out as a leaf and warning once ("a kept UI node was not laid out before; drawn without its contents").
- For every other desc: `entry.below` is the sum over its kids of `1 + below[kid]`, from a per-call `below` buffer filled children-before-parents (a kept kid contributes its `entry.below`).
- The measure callback: Taffy's context id found in `index_of` with `Measure::Text` gives `Measured::Index(i)`; an id not in `index_of` gives `Measured::Kept(id)`; anything else measures zero, as today.

**Rule for `end_frame`:** when `laid != entries.len()`, first walk each node in `self.kept` down through `taffy.children(node)`, and set `frame` on the entry of every descendant (by `taffy.get_node_context(node)`, the `UiId`). Then remove the stale entries as today.

- [ ] **Step 1: Write the failing tests** (in `engine.rs`'s `tests` module; `node(..)` gains `kept: false`, and add a `kept_node` helper):

```rust
  fn kept_node<'s>(id: u64, style: &'s UiStyle) -> NodeDesc<'s> {
    NodeDesc {
      kept: true,
      ..node(id, style, Measure::None, &[])
    }
  }

  /// The spike's measure, given an index or a kept id
  fn measure_any(chars: f32) -> impl FnMut(Measured, (Option<f32>, Option<f32>), (Avail, Avail)) -> Vec2 {
    let mut inner = measure_text(chars);
    move |_, known, avail| inner(0, known, avail)
  }

  /// A 200-wide column (id 1) holding a 180-wide column (id 2) of three texts (ids 3 to 5)
  fn nested<'s>(outer: &'s UiStyle, inner: &'s UiStyle, leaf: &'s UiStyle) -> [NodeDesc<'s>; 5] {
    [
      node(1, outer, Measure::None, &[1]),
      node(2, inner, Measure::None, &[2, 3, 4]),
      node(3, leaf, Measure::Text, &[]),
      node(4, leaf, Measure::Text, &[]),
      node(5, leaf, Measure::Text, &[]),
    ]
  }

  #[test]
  fn a_kept_node_keeps_its_subtree_without_its_children_listed() {
    let outer = UiStyle { width: Length::Px(200.0), ..UiStyle::default() };
    let inner = UiStyle { gap: 4.0, ..UiStyle::default() };
    let leaf = UiStyle::default();
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let full: Vec<Placed> = engine
      .layout(&nested(&outer, &inner, &leaf), Vec2::new(800.0, 600.0), &mut measure_any(40.0))
      .to_vec();
    engine.end_frame();
    let count = engine.node_count();

    engine.begin_frame();
    let kept = [node(1, &outer, Measure::None, &[1]), kept_node(2, &inner)];
    let placed = engine.layout(&kept, Vec2::new(800.0, 600.0), &mut measure_any(40.0)).to_vec();
    engine.end_frame();
    assert_eq!(placed[1].rect, full[1].rect, "the kept node is where it was");
    assert_eq!(engine.node_count(), count, "its subtree is kept");

    engine.begin_frame();
    let _ = engine.layout(&nested(&outer, &inner, &leaf), Vec2::new(800.0, 600.0), &mut measure_any(40.0));
    engine.end_frame();
    let stats = engine.stats();
    assert_eq!((stats.created, stats.rechildren), (0, 0), "its children were left as they were");
  }

  #[test]
  fn the_sweep_keeps_a_kept_subtree_when_something_else_goes() {
    let outer = UiStyle { width: Length::Px(200.0), ..UiStyle::default() };
    let inner = UiStyle::default();
    let leaf = UiStyle::default();
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let with_sibling = [
      node(1, &outer, Measure::None, &[1, 5]),
      node(2, &inner, Measure::None, &[2, 3, 4]),
      node(3, &leaf, Measure::Text, &[]),
      node(4, &leaf, Measure::Text, &[]),
      node(5, &leaf, Measure::Text, &[]),
      node(6, &leaf, Measure::Fixed(Vec2::new(10.0, 10.0)), &[]),
    ];
    let _ = engine.layout(&with_sibling, Vec2::new(800.0, 600.0), &mut measure_any(10.0));
    engine.end_frame();
    let before = engine.node_count();

    engine.begin_frame();
    let kept = [node(1, &outer, Measure::None, &[1]), kept_node(2, &inner)];
    let _ = engine.layout(&kept, Vec2::new(800.0, 600.0), &mut measure_any(10.0));
    engine.end_frame();
    assert_eq!(engine.node_count(), before - 1, "only the gone sibling is removed");
    assert!(engine.holds(UiId(3)) && engine.holds(UiId(5)), "the kept subtree stays");
  }

  #[test]
  fn a_kept_text_leaf_is_measured_by_id_when_taffy_lays_it_again() {
    let outer = UiStyle { width: Length::Fill, ..UiStyle::default() };
    let inner = UiStyle::default();
    let leaf = UiStyle::default();
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let _ = engine.layout(&nested(&outer, &inner, &leaf), Vec2::new(300.0, 600.0), &mut measure_any(60.0));
    engine.end_frame();

    engine.begin_frame();
    let mut asked = Vec::new();
    let mut measure = |target: Measured, known, avail| {
      asked.push(target);
      measure_any(60.0)(target, known, avail)
    };
    let kept = [node(1, &outer, Measure::None, &[1]), kept_node(2, &inner)];
    // Narrower: the texts wrap to more lines, so the kept node grows
    let placed = engine.layout(&kept, Vec2::new(150.0, 600.0), &mut measure).to_vec();
    engine.end_frame();
    assert!(asked.contains(&Measured::Kept(UiId(3))), "a kept leaf is measured by id: {asked:?}");
    assert!(placed[1].rect.height > 51.0, "the kept node follows its texts");
  }

  #[test]
  fn placed_below_matches_a_full_layout() {
    let outer = UiStyle { width: Length::Px(200.0), padding: Edges::all(7.0), ..UiStyle::default() };
    let inner = UiStyle { gap: 4.0, padding: Edges::all(3.0), ..UiStyle::default() };
    let leaf = UiStyle::default();
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let full = engine
      .layout(&nested(&outer, &inner, &leaf), Vec2::new(800.0, 600.0), &mut measure_any(40.0))
      .to_vec();
    let ids = [UiId(2), UiId(3), UiId(4), UiId(5)];
    let parents = [None, Some(0), Some(0), Some(0)];
    let mut out = Vec::new();
    assert!(engine.placed_below(full[1].rect, &ids, &parents, &mut out));
    for (k, i) in (1..5).enumerate() {
      assert_eq!(out[k].rect, full[i].rect, "node {i}");
    }
  }
```

- [ ] **Step 2: Run them to see them fail.**

Run: `cargo test -p ryo2d --lib ui::layout::engine::tests -- --nocapture`
Expected: compile errors (`kept`, `Measured`, `holds`, `placed_below`).

- [ ] **Step 3: Implement** the rules above. In `tree.rs`, the measure closure becomes `|target: Measured, known, avail|`: `Measured::Index(i)` behaves as today; `Measured::Kept(id)` returns `Vec2::ZERO` until Task 4 gives resolve the kept fragments to look leaves up in. Every existing `node(..)` test helper and `NodeDesc` literal gains `kept: false`.

- [ ] **Step 4: Run them to see them pass,** then `cargo test -p ryo2d --lib ui::`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add src/core/ui/layout/engine.rs src/core/ui/tree.rs
git commit -m "feat: the UI layout engine keeps a subtree without walking it"
```

---

### Task 4: Fragments: kept, copied, and resolved again

The walk pushes each outermost clean subtree that has a fragment as one kept node. Resolve copies the fragment's records and commands, re-based to this frame's draw order, when its inputs are unchanged, and otherwise resolves its kept nodes again against Taffy's current layout. A clean subtree with no fragment is replayed as today, and resolve keeps what it resolved to as its fragment. With the switch, the console command and the statistics. From this task on, the node cache oracle (`nodes/oracle.rs`, cache on against off) runs with fragments on.

**Files:**
- Create: `src/core/ui/fragment.rs` (declared in `src/core/ui/mod.rs`)
- Modify: `src/core/ui/tree.rs` (`Node::role`, `DrawKind::Kept`, resolve: capture, copy, resolve again, the kept-leaf measure)
- Modify: `src/core/ui/memory.rs` (the fragment store, the capture logs, the statistics)
- Modify: `src/core/ui/immediate/context.rs` (`push_kept`, `begin_capture`/`end_capture`), `src/core/ui/tape.rs` (`replay` feeds the open capture log)
- Modify: `src/core/ui/nodes/draw.rs`, `src/core/ui/nodes/cache.rs` (`Taken::built`, the walk, `set_fragments_enabled`, the statistics)
- Modify: `src/core/config/debug.rs`, `src/core/engine/mod.rs`, `src/core/engine/frame.rs`, `src/core/console/builtins.rs` (the switch)
- Modify: `src/core/testing/ui.rs` (`set_ui_fragments`, `UiCacheStats` fields), `src/core/ui/nodes/oracle.rs` (`Fragments(bool)` step)
- Test: `src/core/ui/nodes/oracle_tests.rs`, `src/core/ui/nodes/tests*.rs` (where the cache's own tests live), `tests/ui_cache_alloc.rs`

Read first: excerpts for `nodes/draw.rs` (`draw_node`, `replay_taken`, `replay_child`, `build_recorded`), `nodes/cache.rs` (`NodeRecord`, `take_clean`, `replays_here`, `CacheStats`), `tape.rs` (`Tape`, `Op`, `replay`), `context.rs` (`push_stamped`, `push_draw_copy`, `close`, `finish_subtree`, `focus_scope_here`), `memory.rs` (`UiMemory`, `begin_frame`, `restart_pass`, `rewind_frame`, `forget_scene`, `record`, `touch`), and `id.rs` (`replay_id`).

**Interfaces:**
- Consumes: Task 2's slots (`node_slot`, the slot table, rects relative to slots), and Task 3's `NodeDesc::kept`, `Measured::Kept`, `LayoutEngine::holds`/`placed_below`.
- Produces:

```rust
// src/core/ui/fragment.rs
//! Fragments: the outermost clean subtrees of node trees, kept as they were
//! resolved and copied into later frames instead of being replayed, laid
//! out and resolved again

/// How a node takes part in fragments
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Role {
  #[default]
  None,
  /// The root of a clean subtree replayed this frame: what its subtree
  /// resolves to is kept as its fragment
  Capture,
  /// A kept fragment's root, pushed without its subtree: resolve copies
  /// the fragment, or resolves its kept nodes again
  Kept,
}

/// What a fragment's output follows from beyond its own nodes: while these
/// are the same, a copy is what resolving it would give, bit for bit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Inputs {
  /// The subtree's origin (UI units), in bits
  pub(crate) origin: [u32; 2],
  /// The root's rect as laid out, relative to its subtree's root, in bits
  pub(crate) rect: [u32; 4],
  /// The root's layout shift (stack offsets, never a scroll offset), in bits
  pub(crate) shift: [u32; 2],
  /// Its slot's base (render pixels) and the render scale, in bits
  pub(crate) base: [u32; 2],
  pub(crate) scale: u32,
  /// Its clip in slot 0 (absolute), in bits; `None` in a slot, whose clip
  /// applies when it is copied
  pub(crate) clip: Option<[u32; 4]>,
  pub(crate) layer: u8,
}

/// A record of a fragment's node, its rect relative to the fragment's slot
#[derive(Debug, Clone)]
pub(crate) struct KeptRecord {
  pub(crate) id: UiId,
  /// The record as resolved; its rect, clip, order and depth are made again
  /// when it is copied
  pub(crate) record: Record,
  /// Its edges in render pixels, relative to its slot's base: x0, y0, x1, y1
  pub(crate) edges: [f32; 4],
  /// Its order and depth less the fragment root's
  pub(crate) order: u32,
  pub(crate) depth: u16,
}

/// A command of a fragment, relative to its slot, before culling
#[derive(Debug, Clone)]
pub(crate) struct KeptDraw {
  /// Its order less the fragment root's
  pub(crate) order: u32,
  pub(crate) command: UiCommand,
  /// The box it is culled by (`None`: never culled)
  pub(crate) cull: Option<Rect>,
}

/// An outermost clean subtree as resolved
pub(crate) struct Fragment {
  /// The frame its root's node record was built in: a newer record means
  /// the subtree changed
  pub(crate) built: u64,
  pub(crate) scene: u64,
  pub(crate) inputs: Inputs,
  /// The orders its subtree takes (its nodes and draws)
  pub(crate) count: u32,
  /// Its root as pushed (stamped again when pushed) and the rect it was
  /// pushed at
  pub(crate) root: Node,
  pub(crate) at: Option<Rect>,
  /// The ids made and widget states touched by every tape in it
  pub(crate) ids: Vec<UiId>,
  pub(crate) states: Vec<UiId>,
  pub(crate) records: Vec<KeptRecord>,
  pub(crate) draws: Vec<KeptDraw>,
  /// Its nodes as pushed (orders and depths less the root's, parents as
  /// indices into `nodes`, the root's `None`) and their draws (`node` an
  /// index into `nodes`, orders less the root's): to resolve it again
  pub(crate) nodes: Vec<Node>,
  pub(crate) node_draws: Vec<Draw>,
  /// Changes each time it is resolved (captured, or resolved again)
  pub(crate) version: u64,
}

/// What a capture saw while its subtree was replayed
#[derive(Debug, Default)]
pub(crate) struct CaptureLog {
  pub(crate) root: UiId,
  pub(crate) built: u64,
  pub(crate) ids: Vec<UiId>,
  pub(crate) states: Vec<UiId>,
  /// A popup was begun in it: it is never a fragment
  pub(crate) popup: bool,
  /// Its root has been pushed (and marked `Role::Capture`)
  pub(crate) rooted: bool,
}
```

  `UiMemory` gains `fragments_prev: FastMap<UiId, Fragment>`, `fragments_next: FastMap<UiId, Fragment>`, `fragment_version: u64`, `fragment_stats: FragmentStats { captured, reused, resolved_again: usize }`, `fragments_on: bool`, and on `Scratch`, `captures: Vec<CaptureLog>`. `Node` gains `role: Role`; `DrawKind` gains `Kept` (where a kept fragment's commands go in the draw order; never taped, never drawn by `command_of`). `UiContext` gains:

```rust
  /// Push the kept fragment of the clean subtree whose root is `id`, whose
  /// node record was built in frame `built`, as one node in place of a
  /// replay: its ids made and states kept as a replay would, the orders its
  /// subtree takes reserved. False, with nothing pushed, when it has none,
  /// it is stale, or its root would be stamped otherwise than it was.
  pub(crate) fn push_kept(&mut self, id: UiId, built: u64) -> bool;

  /// Start keeping what the replay of the clean subtree rooted at `id`
  /// (record built in frame `built`) makes, touches and pushes
  pub(crate) fn begin_capture(&mut self, id: UiId, built: u64);
  pub(crate) fn end_capture(&mut self);
```

  `UiCacheStats` (testing) gains `pub reused: usize`, `pub resolved_again: usize`, `pub captured: usize`; `UiHarness::set_ui_fragments(&mut self, on: bool)`; `EngineBuilder::ui_fragments(self, on: bool) -> Self`; `DebugConfig::ui_fragments: bool` (default `true`); console `ui:fragments [on|off]`; `nodes::set_fragments_enabled(world, on)`.

**Rules:**

- **The walk** (`draw_node`): when `take_clean` gives a clean node and fragments are on (`cache.enabled && cache.fragments`), with `id` its tape's first node's id (`tape.nodes.first()`; none: replay as today):
  - `ui.push_kept(id, taken.built)` true: count it reused, set `walk.back` as `replay_taken` would (never needed: dialogs are popups), and return `Drawn::Replayed(taken.id)`;
  - otherwise: `ui.begin_capture(id, taken.built)`, `replay_taken(..)`, `ui.end_capture()`.

  `replay_child` (inside a replay) never pushes kept fragments: everything under a fragment's root is the fragment's. `Taken` gains `built: u64`, the `NodeRecord`'s build frame (if `NodeRecord::frame` is updated by a replay rather than set by a build, add a `built` field set only by `build_recorded` and use it).
- **`push_kept`:**
  - It requires all of the following:
    - `fragments_prev` holds `id`, with `built` equal and `scene` the current scene;
    - `self.layer == fragment.inputs.layer`;
    - `self.focus_scope_here() == fragment.root.scope`;
    - the scroll ancestor the root would be stamped with (as `push_stamped` computes it) equals `fragment.root.scroll`;
    - `self.layout.holds(id)`;
    - no tape is recording (`debug_assert!(!self.recording())`).
  - Then it:
    1. makes each of `fragment.ids` with `memory.scopes.replay_id(id)` and touches each of `fragment.states` (`memory.touch(id, scene)`);
    2. pushes a clone of `fragment.root` with `role = Role::Kept` through `push_stamped` at `fragment.at`, so the root takes order `base`;
    3. reserves the rest with `self.order = self.order.saturating_add(fragment.count - 1)`;
    4. pushes a `Draw { order: base, layer, node: index, kind: DrawKind::Kept }` straight onto the subtree's draws (no `next_order`);
    5. calls `self.close(index)`, which finishes the subtree when the root is its root.

  The fragment stays in `fragments_prev`; resolve takes it.
- **The capture log:** `begin_capture` pushes a `CaptureLog { root: id, built, .. }` onto `memory.scratch.captures`; while one is open (the last, `rooted` or not):
  - `UiContext::replay` appends the tape's `ids` and `states`;
  - `push_node_copy` marks the first node it pushes `Role::Capture` and sets `rooted`;
  - `begin_popup` sets `popup`.

  `end_capture` closes it (the log stays in `captures` for resolve). `begin_frame` and `restart_pass` clear `captures`.
- **Resolve.** Before layout, take every `Role::Kept` node's fragment out of `fragments_prev` (`remove`) into a local list. A kept node whose fragment is missing (cannot happen; guard it) is resolved as a leaf with no output, warned once.
  - Describe a kept node with `kept: true` and no children.
  - The measure callback answers `Measured::Kept(id)` from the kept fragments' `nodes` (the node with that id and a text leaf, measured as `Measured::Index` would be). It searches linearly: Taffy asks only when it lays a kept subtree out again.
  - After placement, for each kept node `i`, compute its `Inputs`:
    - `origin`;
    - `placed[i].rect`;
    - `layout_shift[i]`;
    - `base[node_slot[i]]` and `scale`;
    - `clip`: `clip_abs[i]` in slot 0, `None` in a slot;
    - `layer`.
  - **Equal to the fragment's (a copy):**
    - In the records loop, write each `KeptRecord`:
      - `rect`: from `edges` plus the slot's offset, as resolve makes an absolute rect;
      - `clip`: `clip_abs[i]` in slot 0, the slot's clip in a slot;
      - `order`: `base + order`, where `base` is the kept node's own order;
      - `depth`: `node.depth + depth`;
      - the rest as kept.
    - In the draws loop at its `DrawKind::Kept`, push each `KeptDraw` whose `cull` is not `outside(this frame's cull rect, cull)`, as `(base + order, node_slot[i], command.clone())`.
    - Count it `reused`.
  - **Not equal (resolve it again):**
    1. Read its layout with `layout.placed_below(placed[i].rect, ids, parents, ..)` (ids and parents from `fragment.nodes`).
    2. Stamp its kept nodes and draws back to this frame (`order + base`, `depth + node.depth`, parents offset into the run).
    3. Run resolve's per-node placement, records and draws over them, as if they were the subtree's nodes, with the kept root in node `i`'s place: its layout shift, slot, base and clip are node `i`'s.
    4. Rebuild the fragment's `inputs`, `records` and `draws` from that run (this is the capture below, over the kept nodes), bump `version`, and count it `resolved_again`.

    Split resolve's three per-node loops (placement, records, draws) into functions over slices so that both this run and the subtree's own can call them; keep each loop's arithmetic exactly as Task 2 left it.
  - **Captures:** for each `Role::Capture` node `i` whose log (found by root id in `scratch.captures`) has `popup == false`:
    - Its span is `i..i + size[i]`, with `size` from the parents in a reverse pass; a replayed subtree's nodes are contiguous.
    - It is captured only when no node in the span has an `at` rect, no node in it is `Role::Kept`, and every text leaf in it is `Leaf::Shared`.
    - While the records and draws loops run, they also copy:
      - each span node's record into a `KeptRecord`: `edges` relative to its slot, `order` and `depth` less the root's;
      - each draw whose node is in the span into a `KeptDraw`: the command and its cull box, before culling, `order` less the root's.
    - Afterwards build the `Fragment`:
      - `count` is the span's nodes plus its draws;
      - `root` is a clone of node `i` with `role = Role::None`;
      - `at` is node `i`'s `at` when it is the subtree's root, else `None`;
      - `ids` and `states` come from the log;
      - `nodes` and `node_draws` are relative to the root;
      - `inputs` are computed as above, and `version` is a new one.
    - Insert it into `fragments_next`, and count it `captured`.
  - At the end of resolve, insert every kept fragment (copied or resolved again) into `fragments_next`.
- **The store:**
  - `begin_frame` swaps `fragments_prev` and `fragments_next` and clears `fragments_next`, so a fragment not drawn in a frame is gone the frame after;
  - `restart_pass` moves every fragment in `fragments_next` whose `version` it had at the pass's start back into `fragments_prev`, and drops the rest (captured or resolved again in the discarded pass). Record the versions in `fragments_prev` at `begin_frame` (a `FastSet<u64>`, cleared, kept);
  - `rewind_frame` swaps them back as it swaps records;
  - `forget_scene` drops the scene's fragments from both;
  - with fragments off, both are cleared once.
- **The switch:** as `ui_cache`, end to end: `DebugConfig::ui_fragments` (doc: "Whether UI node trees keep the layout and resolved output of unchanged subtrees between frames (default true). Off, unchanged subtrees are replayed and resolved every frame: the same output, slower."), `EngineBuilder::ui_fragments`, `Engine::draw_ui` calls `nodes::set_fragments_enabled(&mut self.world, self.config.debug.ui_fragments)` beside `set_cache_enabled`, the console's `ui:fragments` (copy `ui:cache`'s handler and its test), `UiHarness::set_ui_fragments`.

- [ ] **Step 1: Write the failing tests.** In `src/core/ui/nodes/cache.rs`'s tests (the `#[cfg(test)]` module at its end; the oracle scenarios need the `default-fonts` feature, on by default, as `oracle_tests.rs` does):

```rust
#[test]
fn an_unchanged_tree_is_copied_from_its_fragments() {
  let mut h = UiHarness::new(640.0, 480.0);
  h.spawn_ui(settings_tree());
  h.move_to(-100.0, -100.0);
  for _ in 0..3 {
    h.frame(|_| {});
  }
  let stats = h.ui_cache_stats();
  assert!(stats.reused > 0, "clean subtrees are copied: {stats:?}");
  assert_eq!(stats.captured, 0, "nothing new to keep in an unchanged frame");
  assert!(
    stats.replayed < 20,
    "only the dirty path's own tapes replay, not the screen ({} replayed)",
    stats.replayed
  );
}

#[test]
fn with_fragments_off_the_tree_replays_as_before() {
  let mut h = UiHarness::new(640.0, 480.0);
  h.set_ui_fragments(false);
  h.spawn_ui(settings_tree());
  for _ in 0..3 {
    h.frame(|_| {});
  }
  let stats = h.ui_cache_stats();
  assert_eq!((stats.reused, stats.captured), (0, 0));
  assert!(stats.replayed > 200, "the screen replays");
}

#[test]
fn a_popup_or_a_placed_node_is_never_kept() {
  let tree = UiNode::column()
    .child(UiNode::dropdown(1, ["Low", "High"]).name("pick"))
    .child(UiNode::column().child(UiNode::text("placed").at(Rect::new(300.0, 10.0, 80.0, 20.0))));
  let run = oracle(
    |h| {
      h.spawn_ui(tree.clone());
    },
    &[Frame, Frame, At("pick"), Press, Release, Frame, Frame, Frame],
  );
  // Equal to the uncached harness every frame is the oracle's own check.
  // With the list open, the dropdown replays a popup (never kept) and the
  // column holds a placed node (never kept), so nothing is kept or captured
  let last = run.stats.last().unwrap();
  assert_eq!((last.reused, last.captured), (0, 0), "{last:?}");
}
```

(`UiNode::dropdown(..)` and `.at(..)` stand for the node constructors and options the codebase has for a dropdown and a placed node; read `nodes/node.rs` and use theirs.)

In `oracle_tests.rs`, add `Fragments(bool)` handling to `apply` (`Fragments(on) => if cached { h.set_ui_fragments(*on) }`) and these scenarios:

```rust
#[test]
fn a_rebuilt_node_drops_its_fragment() {
  let run = oracle(
    |h| {
      h.spawn_ui(settings_tree());
    },
    &[
      Frame,
      Frame,
      Frame,
      Write("row 3 label", |world, node| {
        world.get_mut::<UiLabel>(node).unwrap().text = "Changed".into();
      }),
      Frame,
      Frame,
      Frame,
    ],
  );
  assert_cached(&run);
  assert!(run.stats.iter().any(|s| s.reused > 0), "fragments were copied");
}

#[test]
fn a_reconciled_node_drops_its_fragment() {
  // A UI file's tree updated in place, as a hot reload or `update_ui` does
  let run = oracle(
    |h| {
      h.spawn_ui(UiNode::column().name("list").child(UiNode::text("one").name("first")).child(UiNode::text("two")));
    },
    &[
      Frame,
      Frame,
      Frame,
      Do(|h| {
        let list = h.find_ui("list").unwrap();
        h.update_ui(list, UiNode::column().name("list").child(UiNode::text("ONE").name("first")).child(UiNode::text("two")));
      }),
      Frame,
      Frame,
    ],
  );
  assert_cached(&run);
}

#[test]
fn fragments_switched_on_and_off_mid_run() {
  let run = oracle(
    |h| {
      h.spawn_ui(settings_tree());
    },
    &[Frame, Frame, Fragments(false), Frame, Frame, Fragments(true), Frame, Frame, Frame],
  );
  assert_cached(&run);
}

#[test]
fn a_fragment_moved_by_a_growing_sibling_is_resolved_again() {
  let run = oracle_with(
    |h| {
      h.spawn_ui(
        UiNode::column()
          .child(UiNode::text("").name("grows").bind_text("{n}"))
          .child(UiNode::row().child(UiNode::button("Kept")).child(UiNode::button("Also kept"))),
      );
    },
    &|_, _, _| {},
    &|_| {},
    &[
      Frame,
      Frame,
      Frame,
      Set("grows", "n", BindValue::from("a\nlonger\ntext\nthat\nwraps")),
      Frame,
      Frame,
    ],
  );
  assert_cached(&run);
  assert!(run.stats.iter().any(|s| s.resolved_again > 0), "the moved row was resolved again");
}
```

(Adapt the names to the fixtures: `"row 3 label"` must name a node in `settings_tree()`, so read `testing/fixtures.rs` and pick a label it names, or name one; `bind_text`/`BindValue::from` stand for the bindings API in `nodes/bindings.rs`; `h.update_ui` for the harness's update method. Keep what each scenario does.)

In `tests/ui_cache_alloc.rs`: the unchanged cached frame's count stays within today's (14); print it.

- [ ] **Step 2: Run them to see them fail.**

Run: `cargo test -p ryo2d --lib nodes:: -- --nocapture`
Expected: compile errors (`set_ui_fragments`, `reused`, `Fragments`).

- [ ] **Step 3: Implement** by the rules above, in this order: the types (`fragment.rs`), the store and logs (`memory.rs`), `Node::role` and `DrawKind::Kept`, `push_kept` and the captures (`context.rs`, `tape.rs`), resolve (`tree.rs`), the walk (`draw.rs`, `cache.rs`), the switch, the statistics.

- [ ] **Step 4: Run them to see them pass,** then every node test (the 21 oracle scenarios now run with fragments on):

Run: `cargo test -p ryo2d --lib nodes:: ui:: && cargo test --test ui_cache_alloc -- --nocapture && cargo test --workspace`
Expected: PASS, the allocation count at most 14.

- [ ] **Step 5: Measure** `cargo bench --bench ui -- 'settings screen as nodes'` and put the medians beside Task 1's in your report (no target is asserted until Task 10).

- [ ] **Step 6: Commit.**

```bash
git add src/core/ui/fragment.rs src/core/ui/mod.rs src/core/ui/tree.rs src/core/ui/memory.rs src/core/ui/immediate/context.rs src/core/ui/tape.rs src/core/ui/nodes/draw.rs src/core/ui/nodes/cache.rs src/core/config/debug.rs src/core/engine/mod.rs src/core/engine/frame.rs src/core/console/builtins.rs src/core/testing/ui.rs src/core/ui/nodes/oracle.rs src/core/ui/nodes/oracle_tests.rs src/core/ui/nodes/cache.rs tests/ui_cache_alloc.rs
git commit -m "feat: unchanged UI subtrees are copied from their kept fragments"
```

---

### Task 5: A scenario for every input a fragment depends on

Each input in the spec's list gets an oracle scenario that a fragment ignoring that input would fail, plus the Review Focus tests 1 and 2. For each, show it failing by breaking the check it covers (temporarily: compare one field fewer in `Inputs`, or skip one `push_kept` condition), then restore the check and watch it pass. Report each pairing (scenario → the check it caught).

**Files:**
- Modify: `src/core/ui/nodes/oracle_tests.rs` (the scenarios), `src/core/ui/nodes/oracle.rs` (any step a scenario needs)
- Test: those, plus `tests/e2e/ui_fragments.rs` (create) for the engine-level scenarios (scene push, rerun pass)

**Interfaces:**
- Consumes: Task 4's `UiCacheStats { reused, resolved_again, captured }`, `Fragments(bool)`.

The scenarios (each in `oracle_tests.rs` unless marked e2e; each asserts `assert_cached(&run)` and that fragments were reused in it, `run.stats.iter().any(|s| s.reused > 0)`):

| Scenario | What it does | The check it pins |
|---|---|---|
| `a_sibling_inserted_before_a_fragment` | a closure label drawn before the tree appears at frame 3 (the scenario's `ui` draws it from `frame >= 3`), so the tree's orders move by one | order re-basing (`base`) |
| `a_growing_sibling_moves_a_fragment_by_a_fraction_then_whole_pixels` | a bound text above a row grows by a fraction of a pixel, then by a whole line | `Inputs::rect` and `origin` |
| `a_window_resize` | `Resize(500.0, 400.0)` then back | `Inputs::rect`, `scale`, `base`; Taffy's re-layout |
| `a_resize_measures_text_inside_a_kept_fragment` | a fill-width column of wrapping texts, kept; `Resize` narrower so the texts wrap to more lines, without the column's root changing size until they do | `Measured::Kept` (Review Focus 1): without it the texts measure zero |
| `a_clip_change` | the scenario's `ui` wraps `ui.node(root)` in `push_clip`/`pop_clip` with a clip that changes at frame 3 (an embedded tree) | `Inputs::clip` |
| `a_layer_change` | the embedded tree drawn on another layer from frame 3 (`set_layer`) | `Inputs::layer` |
| `the_top_scene_changes` (e2e) | a page with a node tree; a transparent scene pushed over it at frame 5, popped at frame 10 | `scene`/top in `push_kept` |
| `scene_pushed_over_fragments` (e2e) | as above, with the top scene's closure label changing every frame, so the lower scene's tree keeps its fragments while the orders before it stay and the top scene's grow; a click on the top scene lands on it (Review Focus 2) | order base across scenes; hit tests by layer and order |
| `each_env_change` | `Theme(..)`, `Device(..)`, `Resize` in turn | the record's env (fragments follow their records) |
| `nested_slots_by_wheel_and_trackpad` | the Clipping page's shape: a list holding a horizontal strip of cards; wheel and fractional trackpad steps on both, at UI scale 1.25 (`Do(|h| h.set_ui_scale(1.25))`) | slots, `base`, the cull rect |
| `a_popup_opened_next_to_a_fragment` | a dropdown beside a kept column opened and closed | popups are never kept; orders after a popup |
| `an_embedded_tree_in_closure_ui` | `ui.node(root)` inside a closure grid whose other cells change | the walk inside closure UI |
| `virtual_list_rows` | a node list scrolled by `Jump` and the wheel, rows rebound | rows as fragments in a list's slot |
| `a_rerun_pass` (e2e) | a scene whose `ui` switches scene at frame 4, so the pass runs twice that frame | `restart_pass` keeps only the last frame's fragments |

- [ ] **Step 1: Write the scenarios** in the table, then run them all (`cargo test -p ryo2d --lib oracle_tests` and `cargo test --test e2e ui_fragments`). Expected: PASS.
- [ ] **Step 2: For each scenario, break its check and watch it fail.** Report the failure message, then restore the check. A scenario that does not fail with its check broken does not test it: change the scenario until it does.
- [ ] **Step 3: Run** `cargo test --workspace`. Expected: PASS.
- [ ] **Step 4: Commit.**

```bash
git add src/core/ui/nodes/oracle_tests.rs src/core/ui/nodes/oracle.rs tests/e2e/ui_fragments.rs tests/e2e/main.rs
git commit -m "test: an oracle scenario for each input a UI fragment depends on"
```

(`tests/e2e/main.rs` or whatever declares the e2e modules: add `mod ui_fragments;` there.)

---

### Task 6: Interest-rect culling, the fragment spot check and the overlay

A fragment's run under a slot is culled against the slot's interest rect, so a scrolling list's runs change only every half view; the oracle compares commands after culling both sides to their clips; debug builds check one reused fragment a frame against a fresh resolve; the `debug:ui-rebuilds` overlay counts fragments.

**Files:**
- Modify: `src/core/ui/tree.rs` (the interest rect per slot; a kept fragment's cull), `src/core/ui/fragment.rs` (`KeptDraw` culled by interest; the fragment remembers the interest it was last emitted under)
- Modify: `src/core/ui/immediate/commands.rs` (`UiCommands::visible()` for snapshots), `src/core/testing/ui.rs` (`ui_snapshot` prints the visible draws)
- Modify: `src/core/ui/nodes/draw.rs`, `src/core/ui/nodes/cache.rs` (the fragment spot check), `src/core/ui/debug.rs` (the counts)
- Test: `src/core/ui/nodes/oracle_tests.rs`, the cache tests, `src/core/ui/debug.rs` tests

**Interfaces:**
- Produces:
  - `UiCommands::visible(&self) -> impl Iterator<Item = &UiDraw>`: the draws that can show a pixel, each tested by its box (rect, sprite rect, text block, a line's points' bounds widened by its width) moved by its slot's offset against its slot's clip intersected with its own;
  - `UiCacheStats::spot_fragment: Option<UiId>` and `UiCacheStats::spot_fragment_differed: usize`;
  - the overlay's second line reads `fragments: N kept, M resolved again, K captured` beside 4f's counts (refreshed once a second, as 4g made its lines).

**Rules:**
- **The interest rect,** per slot `k > 0`, made when the slot is made, in the slot's own coordinates (UI units). Along each axis the area scrolls:
  - `view` is the area's view size and `d` its drawn offset in UI units;
  - `step = view / 2`;
  - `start = (d / step).floor() * step - view`;
  - the rect runs from `start` to `start + 3 * view`.

  Along an axis it does not scroll, it is the view itself.
- **A kept fragment's draws in a slot** are culled against the slot's interest rect instead of its view; in slot 0, against its clip as before.
- **The fragment's version** changes when the interest rect it is emitted under differs from the one it was last emitted under: keep `interest: Option<[u32; 4]>` on the fragment. So the renderer sees a new run only every half view.
- **Loose draws** (everything not in a fragment) are culled as Task 2 left them.
- **The snapshot** prints `visible()` draws (with their slots), not every draw, so fragments on and off compare equal.
- **The spot check:**
  - Each frame in debug builds while it is on, the fragment that comes next in a stable order is not pushed kept; it is replayed and captured instead. Keep the frame's spot root in `UiCache` like 4f's `spot_due`.
  - Its old fragment is held aside, and after resolve the new capture is compared with it: `records`, `draws` and `count`, by `Debug` form.
  - A difference warns once (`"a kept UI fragment differs from its node's resolve: {node} {first difference}"`) and counts in `UiCacheStats::spot_fragment_differed` (4f's record spot check keeps its own counter).
  - The oracle asserts it stays 0, as it asserts 4f's.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn a_scrolling_lists_fragments_change_only_every_half_view() {
  let mut h = UiHarness::new(640.0, 480.0);
  h.spawn_ui(settings_tree());
  h.frame(|_| {});
  h.move_to_settings_list(); // the pointer over a row of the list
  let mut versions = Vec::new();
  for _ in 0..20 {
    h.trackpad(0.0, -3.0);
    h.frame(|_| {});
    versions.push(h.fragment_versions());
  }
  let changes = versions.windows(2).filter(|w| w[0] != w[1]).count();
  assert!(changes <= 2, "60 units of travel in a 407-unit view crosses at most one step ({changes})");
}

#[test]
fn the_spot_check_compares_a_kept_fragment_with_its_resolve() {
  let mut h = UiHarness::new(640.0, 480.0);
  h.set_ui_spot_check(true);
  h.spawn_ui(settings_tree());
  let mut checked = 0;
  for _ in 0..12 {
    h.frame(|_| {});
    checked += usize::from(h.ui_cache_stats().spot_fragment.is_some());
    assert_eq!(h.ui_cache_stats().spot_fragment_differed, 0);
  }
  assert!(checked >= 8, "a fragment is checked most frames ({checked})");
}
```

(`move_to_settings_list` and `fragment_versions` are `#[cfg(test)]` helpers you add: the pointer to the centre of the settings list's view, and the sorted `(root id, version)` of `fragments_next`.)

Run every oracle scenario `with_spot_checks` (4f's `oracle_tests.rs` already has such a test over its scenario list): add Task 5's scenarios to that list.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p ryo2d --lib nodes:: -- --nocapture`). Expected: compile errors, then the version test fails (the run changes every frame without interest culling).
- [ ] **Step 3: Implement** the rules.
- [ ] **Step 4: Run them to see them pass,** then `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add src/core/ui/tree.rs src/core/ui/fragment.rs src/core/ui/immediate/commands.rs src/core/testing/ui.rs src/core/ui/nodes/draw.rs src/core/ui/nodes/cache.rs src/core/ui/debug.rs src/core/ui/nodes/oracle_tests.rs src/core/ui/nodes/cache.rs
git commit -m "feat: fragments cull by a scroll slot's interest, with a spot check and overlay counts"
```

---

### Task 7: Fragment runs reach the renderer, and the image is kept by run

Each fragment's commands reach `UiCommands` as one tagged run; `UiRetain::keep` compares a run by its tag instead of command by command.

**Files:**
- Modify: `src/core/ui/immediate/commands.rs` (`FragmentRun`, `UiCommands::runs`, `push_run`)
- Modify: `src/core/ui/tree.rs` (a kept or captured fragment's commands pushed as a run)
- Modify: `src/core/render/ui_retain/mod.rs` (`keep` by run)
- Test: `src/core/render/ui_retain/tests.rs`, the node cache tests

Read first: excerpts `src/core/render/ui_retain/mod.rs:1-146` (`keep`, `same`, `stamp_of`, `animated`).

**Interfaces:**
- Produces:

```rust
/// A fragment's commands in the frame: `draws` is their range in
/// `UiCommands::as_slice()` (contiguous, in painter's order, one layer, one
/// slot), and the rest says when they are the same as before
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FragmentRun {
  /// Its root's id
  pub(crate) id: UiId,
  /// Changes whenever its commands may (`Fragment::version`)
  pub(crate) version: u64,
  /// Its first order: the run's orders are `base..base + count`
  pub(crate) base: u32,
  pub(crate) layer: u8,
  pub(crate) slot: u16,
  pub(crate) draws: std::ops::Range<u32>,
}
```

  `UiCommands::push_run(&mut self, run: FragmentRun)` (the caller pushes the run's draws first, then the run with their range); `UiCommands::runs(&self) -> &[FragmentRun]`; `clear()` clears runs.

**Rules:**
- **Resolve:** in the draws loop, a kept fragment (copied or resolved again) pushes its draws, then its run with `version` = the fragment's; a fragment captured this frame does the same with its new version. Draws outside fragments push no run.
- **`keep`:** a run is the same as last frame's when its `FragmentRun` (id, version, base, layer, slot) equals the run at the same place in last frame's run list, **and** none of its commands is `animated`. Keep a per-run `animated: bool` computed once when the run is first seen with that version, and keep each run's stamps (sprite and icon versions) the same way.
  - Loose draws are compared one by one as today (orders, slots, `same`, stamps).
  - The slot table is compared whole.
  - For a run, store its tag and stamps, not copies of its commands. The fragment keeps its commands, and with them its text layouts and points, alive while its version is the same.
- **The snapshot** (`ui_snapshot`) prints the runs' tags after the slots, so a missing or doubled run shows in the oracle. With fragments off there are no runs, so print the runs only from a second snapshot field that the oracle leaves out of its comparison (`frame_snapshot` compares everything but `runs`). Print them in the e2e logs.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn an_unchanged_run_is_kept_without_comparing_its_commands() {
  let mut retain = UiRetain::default();
  let env = test_env();
  let frame = |version: u64| {
    let mut commands = UiCommands::default();
    commands.clear();
    let start = commands.as_slice().len() as u32;
    commands.push(10, 0, rect_command(Rect::new(0.0, 0.0, 10.0, 10.0)));
    commands.push(11, 0, rect_command(Rect::new(0.0, 20.0, 10.0, 10.0)));
    commands.push_run(FragmentRun { id: UiId(7), version, base: 10, layer: 0, slot: 0, draws: start..start + 2 });
    commands
  };
  assert!(!retain.keep_frame(&frame(1), &env), "drawn the first time");
  retain.presented_for_test();
  assert!(retain.keep_frame(&frame(1), &env), "the same run is kept");
  assert!(!retain.keep_frame(&frame(2), &env), "a new version is drawn");
}

#[test]
fn a_kept_tree_pushes_one_run_per_fragment() {
  let mut h = UiHarness::new(640.0, 480.0);
  h.spawn_ui(settings_tree());
  for _ in 0..3 {
    h.frame(|_| {});
  }
  let commands = h.commands();
  let runs = commands.runs();
  assert_eq!(runs.len(), h.ui_cache_stats().reused, "one run per kept fragment");
  for run in runs {
    let draws = &commands.as_slice()[run.draws.start as usize..run.draws.end as usize];
    assert!(draws.iter().all(|d| d.slot == run.slot && d.order >= run.base), "{run:?}");
    assert!(draws.windows(2).all(|w| w[0].order < w[1].order), "in order");
  }
}
```

(`keep_frame` and `presented_for_test` stand for `keep(..)` with the test module's icons and image epoch, and `presented(false, true, epoch)`: use the existing test helpers.)

- [ ] **Step 2: Run them to see them fail** (`cargo test -p ryo2d --lib ui_retain run -- --nocapture`). Expected: compile errors.
- [ ] **Step 3: Implement** the rules.
- [ ] **Step 4: Run them to see them pass,** then `cargo test --workspace` (4g's retention oracle in `tests/e2e/ui_retain.rs` and the demo and breakout retain tests must still pass). Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add src/core/ui/immediate/commands.rs src/core/ui/tree.rs src/core/render/ui_retain/mod.rs src/core/render/ui_retain/tests.rs src/core/testing/ui.rs src/core/ui/nodes/cache.rs
git commit -m "feat: UI fragments reach the renderer as runs, and the image is kept by run"
```

---

### Task 8: Scroll slots in the shader

The vertex shader adds a slot's whole-pixel offset and applies its clip, so instances in a slot are built relative to it and do not change while it scrolls. A quad wholly outside its clip is collapsed in the vertex stage.

**Files:**
- Modify: `src/core/render/ui_renderer/instance.rs` (the slot bits), `build.rs` (instances in a slot built relative, culled against the slot's clip moved by its offset), `gpu.rs` (the slot uniform buffer and its bind group, group 2), `mod.rs` (upload the slots each frame; draw binds group 2)
- Modify: `src/core/shaders/core/ui.wgsl`
- Test: `src/core/render/ui_renderer/tests.rs`, `src/core/render/ui_renderer/gpu_tests.rs`

Read first: excerpts for `instance.rs` (whole), `ui.wgsl` (whole), `quad.wgsl`, `gpu.rs` (`upload`, the pipeline layout), `mod.rs` (`finish`, `UiFrame::draw`).

**Interfaces:**
- Produces:
  - `UiInstance::quad(kind, instance, clip, nearest, page)` and `UiInstance::triangle(..)` keep their signatures and gain `with_slot(self, slot: u8) -> UiInstance`, which sets bits 8..16 of `meta.w`'s low half (`flags | slot << 8`); `UiInstance::slot(&self) -> u8`.
  - In `gpu.rs`:

```rust
/// One slot as the shader reads it (16-byte aligned for a uniform array)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct SlotUniform {
  /// The offset in UI units (render pixels / px_scale), as the CPU path adds it
  pub(crate) offset: [f32; 2],
  pub(crate) _pad: [f32; 2],
  /// The clip in UI image pixels: x0, y0, x1, y1 (as `ClipRect`)
  pub(crate) clip: [u32; 4],
}
pub(crate) const SLOTS: usize = 256;
```

  `UiGpu` owns a `SLOTS * 32`-byte uniform buffer and a bind group for it (group 2, vertex and fragment visibility), written once a frame with `write_buffer` (only the slots used: `slots.len() * 32` bytes).

**Rules:**
- **Building** (`build_command`):
  - A draw in slot `k` with `1 <= k < SLOTS` is built from its command as it is (relative to the slot), with its instance's `ClipRect` the slot's clip in image pixels (`ClipRect::of(scissor_of(slot.clip, space)..)`, as the scissor would be), and `with_slot(k as u8)`.
  - It is culled against that clip with its box moved by the slot's offset (the same `Seen::sees`, on the moved box).
  - A draw in a slot at or past `SLOTS` is built absolute, as Task 2 did on the CPU: moved, slot bits 0.
  - Slot 0 is unchanged.
- **The uniform:** slot `k`'s `offset` is `Vec2::from(slots[k].offset) / px_scale`, the exact value Task 2's CPU path added, and `clip` is its pixel clip. Slot 0 is zero offset and the whole image.
- **`ui.wgsl`:**
  - `@group(2) @binding(0) var<uniform> slots: array<Slot, 256>;`
  - In `vs_ui`, read `slot = (meta.w >> 8u) & 0xffu`.
  - For a quad, add `slots[slot].offset` to the corner in UI space before the camera transform, and before the pixel-art snap, so a snapped target snaps the moved corner as the CPU path did. For a line triangle, add it to each corner.
  - Pass the clip on to the fragment shader as today, from the instance (for slot 0) or from `slots[slot].clip`.
  - Collapse the primitive when its corners' bounding box (computed for all four corners of a quad, or the three of a triangle, in image pixels) lies wholly outside the clip. Put the vertex at the same point for every corner, outside the clip.
  - The fragment shader's clip test is unchanged.
- **Exactness:** the pixel oracle (fragments on against off) is unaffected, as both sides take the same path. Against Task 7's build, the pixels must be identical too: the offset is added in the same `f32` arithmetic. The tour must still match the baseline.

- [ ] **Step 1: Write the failing tests.** In `tests.rs`: a draw in slot 1 (`offset [0, 12]`, clip `(0, 10, 100, 50)`) builds an instance with `slot() == 1`, its rect's position as the command gives it (not moved), and its clip the slot's in pixels; a draw in slot 300 of a 301-slot table builds moved, `slot() == 0`. In `gpu_tests.rs` (GPU, skipped without one): draw a 20×20 rect at `(0, 0)` in a slot with `offset [5, 9]` and clip `(0, 0, 64, 64)` into a 64×64 image, and the same rect at `(5, 9)` in slot 0. The two images are identical pixel for pixel. Then the slot's clip `(0, 0, 10, 10)` cuts it to the 5×1 pixels inside.
- [ ] **Step 2: Run them to see them fail** (`cargo test -p ryo2d --lib ui_renderer -- --nocapture`). Expected: compile errors, then the GPU test fails (the slot is not applied).
- [ ] **Step 3: Implement** the rules.
- [ ] **Step 4: Run them to see them pass,** then `cargo test --workspace`, `./dev demos:tour` against the baseline (the Task 2 command; expected: no page differs), and `cargo check -p ryo2d --target wasm32-unknown-unknown`.
- [ ] **Step 5: Commit.**

```bash
git add src/core/render/ui_renderer/instance.rs src/core/render/ui_renderer/build.rs src/core/render/ui_renderer/gpu.rs src/core/render/ui_renderer/mod.rs src/core/shaders/core/ui.wgsl src/core/render/ui_renderer/tests.rs src/core/render/ui_renderer/gpu_tests.rs
git commit -m "feat: the UI shader moves scroll slots' instances by the slot's offset"
```

---

### Task 9: The renderer keeps each run's instances

A fragment run's instances are built once per version and copied in later frames; a run under a slot is built unculled (the pass already culled it by the slot's interest), so it stays valid while the slot scrolls.

**Files:**
- Create: `src/core/render/ui_retain/fragments.rs` (`UiRunCache`)
- Modify: `src/core/render/ui_renderer/build.rs` (`build_ui` copies a cached run, or builds and stores it), `src/core/render/ui_renderer/mod.rs` (the cache lives in the renderer; `begin`/`end` like the text cache)
- Test: `src/core/render/ui_renderer/tests.rs`, `src/core/render/ui_retain/gpu_tests.rs`

Read first: excerpts for `build.rs` (whole: `UiBuild`, `build_ui`, `build_command`, `build_line`, `TextInstances`), `texts.rs` (`UiTextCache`: how it pins glyph pages and checks their generations, and how `begin`/`end` drop entries), `ui_retain/mod.rs` (`stamp_of`).

**Interfaces:**
- Produces:

```rust
/// A fragment run's instances as built, to copy while the run is the same
pub(crate) struct KeptRun {
  pub(crate) version: u64,
  /// Built under this environment
  pub(crate) env: UiRenderEnv,
  pub(crate) instances: Vec<UiInstance>,
  pub(crate) needs: Vec<TexNeed>,
  /// Its sprites, `index` relative to its first instance
  pub(crate) sprites: Vec<SpriteFix>,
  /// The sprite textures its commands asked for, `command` relative to its
  /// first draw, in the order asked
  pub(crate) resident: Vec<(u32, TextureId)>,
  /// The glyph pages it samples and their generations when built
  pub(crate) pages: Vec<(GlyphPage, u64)>,
  /// Its commands' stamps (sprite and icon versions) when built
  pub(crate) stamps: Vec<u64>,
  /// A command reads the clock (a text with an effect): built every frame
  pub(crate) animated: bool,
}

/// Every run's kept instances, by root id; a run not drawn in a frame, or
/// built under another environment, is dropped
#[derive(Default)]
pub(crate) struct UiRunCache { /* FastMap<UiId, KeptRun>, the frame's seen set */ }
```

  `UiRunCache::begin(&mut self, env: UiRenderEnv, retain: bool)`, `get(&self, run: &FragmentRun, pages: impl Fn(GlyphPage) -> u64, stamps: impl Fn(u32) -> u64) -> Option<&KeptRun>` (`None` when stale), `store(&mut self, id: UiId, run: KeptRun)`, `end(&mut self)` (drops runs not seen). `stats()` reports the runs copied and built (add both to `UiRenderStats`: `runs_copied`, `runs_built`).

**Rules:**
- **`build_ui`** walks the commands in painter's order as today. Where a run starts and is in order (runs are contiguous; when the frame is out of order and sorted, build every command as today, uncached):
  - **A valid `KeptRun`:** append its instances and needs. Append its sprite fixes with `index` moved to where its first instance landed, and its resident textures with `command` moved to the run's first draw's index. Skip its commands. Count it copied.
  - **Otherwise:** build its commands. Under a slot, nothing is culled: not the command, not a glyph, not a line triangle (the vertex shader collapses what lies outside). In slot 0, cull as today. Then store a `KeptRun` from what they appended, and count it built.
- **Validity:**
  - the same `version` and env;
  - every page in `pages` still has the same generation (the text cache's check: an evicted, cleared page has a new one);
  - every command's current stamp equals its kept one (`stamp_of`);
  - not `animated`.
- With retention off, the cache is cleared and every run is built every frame (as `ui:retain off` builds everything).
- **The sprite fix** (`fix_sprites`) runs after, over every sprite, kept or built: a texture's slot can move, and the fix reads it each frame.

- [ ] **Step 1: Write the failing tests.**
  - In `tests.rs` (CPU build):
    - **The second frame copies the run:** build the same commands with one run twice. The second build reports `runs_copied == 1, runs_built == 0` and gives the same instances.
    - **A new version rebuilds it:** with a new version the run is built again.
    - **A slot's run is unculled:** a run in a slot whose clip shows only its first command still builds instances for all of its commands; loose draws in that slot are culled.
  - In `ui_retain/gpu_tests.rs`: `an_evicted_glyph_page_drops_a_fragments_instances` (Review Focus 5).
    1. A kept tree's text sits on a glyph page.
    2. Fill the atlas from world text until that page is evicted (4g's glyph-pressure test shows how).
    3. On the next frame the run is built, not copied, and the frame's pixels equal those with fragments off (`PixelPair::fragments`, added in Task 10; until then, two `HeadlessEngine`s built with `ui_fragments(true)` and `(false)`).
- [ ] **Step 2: Run them to see them fail** (`cargo test -p ryo2d --lib ui_renderer ui_retain -- --nocapture`). Expected: compile errors.
- [ ] **Step 3: Implement** the rules.
- [ ] **Step 4: Run them to see them pass,** then `cargo test --workspace` and the tour against the baseline. Expected: PASS, no page differs.
- [ ] **Step 5: Measure** `cargo bench --bench ui_render` and report the render step's medians beside Task 1's.
- [ ] **Step 6: Commit.**

```bash
git add src/core/render/ui_retain/fragments.rs src/core/render/ui_retain/mod.rs src/core/render/ui_renderer/build.rs src/core/render/ui_renderer/mod.rs src/core/render/ui_renderer/tests.rs src/core/render/ui_retain/gpu_tests.rs
git commit -m "feat: the UI renderer copies an unchanged fragment run's instances"
```

---

### Task 10: The pixel oracles, the guards and the targets

Every ui demo page and breakout, fragments on against off, frame by frame, with wheel and trackpad scrolling at three UI scales and the render cases; the targets asserted.

**Files:**
- Modify: `src/core/testing/retain.rs` (`PixelPair::fragments`)
- Create: `examples/demos/ui/tests/fragments.rs`, `examples/games/breakout/tests/fragments.rs`, and add to `tests/e2e/ui_fragments.rs`
- Modify: `benches/ui.rs`, `benches/ui_render.rs` (the targets asserted), `tests/ui_cache_alloc.rs`
- Modify: `docs/backlog/ui-retained-nodes.md` (a "4i: fragments (measured)" section)

**Interfaces:**
- Produces: `PixelPair::fragments(make: impl Fn() -> EngineBuilder) -> Option<PixelPair>`: `on` built with `ui_fragments(true)`, `off` with `ui_cache(false)` (a full rebuild), both with retention on. `frames` and the counters work as for `new`; `kept()`/`reused()` keep their meaning, and a new `fragments_reused()` sums `on`'s node walk's `reused`.

**Rules and tests:**
- **`examples/demos/ui/tests/fragments.rs`:** the shape of `retain.rs` in the same folder (every page: opening, still, moving the focus, pointing, the wheel, pressing, the modal on the Clipping page), with `PixelPair::fragments`. After the wheel step add a trackpad step, `pair.each(|e| e.trackpad(0.0, -7.3))` then 10 frames, and run every page at UI scale 1 and 1.25 (`builder.ui_scale(1.25)`); run pages 1 (Settings), 7 (Clipping) and 17 (Big list) at 2 as well. Assert `pair.fragments_reused() > 0` over the run, and that every page draws in at most 2 UI draws, as `retain.rs` does.
- **`hover_across_a_trackpad_scrolled_list_at_1_25`** (Review Focus 4): the Big list page at 1.25.
  1. Trackpad steps of -3.7, -5.1 and -2.9 bring the list to a fractional offset.
  2. Move the pointer across eight rows, one row a frame.
  3. Every frame matches.
- **`examples/games/breakout/tests/fragments.rs`:** the menu, the quit confirm, play with the HUD, pause and the return to the menu, as its `retain.rs` drives them, with `PixelPair::fragments`.
- **`tests/e2e/ui_fragments.rs`** gains the render cases, each with `PixelPair::fragments`:
  - a sprite updated in place under a kept fragment (`assets().update_sprite` or what 4g's sprite case used);
  - a font render change (MSDF on and off);
  - a tree with 300 scroll areas, so slots run past 256.
- **The guards:**
  - **`benches/ui.rs`:** assert the unchanged node screen under 15 µs, focus moving every frame under 18 µs, the 2,000-node static tree under 100 µs, and the closure screen within 5% of its Task 1 median (read the medians from criterion's `estimates.json`, or time 2,000 frames in the bench's setup, as the 4f guard does).
  - **`benches/ui_render.rs`:** assert the engine UI pass medians: idle under 18 µs, one label under 25 µs, scrolling under 20 µs. Assert the render step over empty: one label under 25 µs, scrolling under 20 µs, idle under 5 µs.
  - A missed target prints the profile command to run and fails. Do not loosen a target: report it with a profile (`xcrun xctrace record --template 'Time Profiler'`, attributed as `profile.md` in the research folder does), and the owner decides.
- **`tests/ui_cache_alloc.rs`:** the unchanged cached frame's allocations stay at or below 14.
- **The backlog:** a "4i: fragments (measured)" section in `docs/backlog/ui-retained-nodes.md`, written in its style:
  - the Task 1 and final medians for every bench above;
  - the oracle's frame counts;
  - a profile of the final label and scrolling frames, against Task 1's buckets (`profile.md`).

- [ ] **Step 1: Add `PixelPair::fragments`** and the tests above; run them (`cargo test -p ui --test fragments`, `cargo test -p breakout --test fragments`, `cargo test --test e2e ui_fragments`). Expected: PASS. A failure is a bug in Tasks 2 to 9: find it with the oracle's first differing frame (the saved PNGs) and fix it in this task, with a test at the level it belongs to.
- [ ] **Step 2: Add the guards** and run `cargo bench --bench ui` and `cargo bench --bench ui_render` twice, at quiet load (report `uptime`). Expected: every target met; otherwise stop and report as above.
- [ ] **Step 3: Write the backlog section.**
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add src/core/testing/retain.rs examples/demos/ui/tests/fragments.rs examples/games/breakout/tests/fragments.rs tests/e2e/ui_fragments.rs benches/ui.rs benches/ui_render.rs tests/ui_cache_alloc.rs docs/backlog/ui-retained-nodes.md
git commit -m "test: UI fragments draw every demo page and breakout as a full rebuild does, and meet their targets"
```

---

### Task 11: The docs and the scrolling review

**Files:**
- Modify: `src/core/ui/AGENTS.md`, `src/core/render/AGENTS.md`, `.claude/skills/ryo-ui/SKILL.md` and `src/cli/skills/ryo-ui.md` (byte-identical, version bumped), `.claude/skills/ryo-renderer/SKILL.md` and `src/cli/skills/ryo-renderer.md` (the same)

**What each says** (write as the files are written: each fact with its reason, no history; the spec is the record of decisions):
- **`src/core/ui/AGENTS.md`:**
  - **A "Fragments" section after "Cached node trees":**
    - the unit (an outermost clean subtree, found by the cache's dirty rules);
    - what is never one (popups, live nodes, a subtree with an `at` node);
    - what a fragment keeps (layout in the layout engine, records and commands relative to its slot, the kept nodes);
    - the copy and the resolve again, with the inputs each checks and why each input matters;
    - draw order (contiguous, reserved, re-based);
    - records carried, ids made and states kept as a replay does;
    - the store's lifetime (two frames, rerun passes, rewinds, scenes);
    - interest-rect culling;
    - the spot check; the switch and the overlay counts;
    - "a new input that changes a fragment's output without dirtying its node gets an `Inputs` field and an oracle scenario that fails without it".
  - **"Scrolling" rewritten:** the drawn offset in whole render pixels, slots (base, offset, clip, nesting), positions relative to slots computed without scroll offsets, records absolute, `at` nodes in slot 0.
- **`src/core/render/AGENTS.md`, in "The UI Renderer":**
  - draws carry slots;
  - the slot uniform (group 2) and the offset added before the camera and the pixel-art snap;
  - vertex-stage culling;
  - fragment runs and the run cache, its validity checks and why a run under a slot is unculled;
  - `keep` by run.
- **`ryo-ui` skill, performance notes:**
  - a frame costs about what changed: unchanged subtrees of node trees are copied, closure UI is built every frame;
  - what makes a subtree rebuild (its components, hover, press and focus, bindings, a live scroll area or list above it);
  - scrolling moves content by whole render pixels;
  - `ui:fragments` and `EngineBuilder::ui_fragments` for ruling fragments out.
- **`ryo-renderer` skill:** the slot uniform and runs, briefly, beside 4h's notes.

- [ ] **Step 1: Write the docs.** Check the skills' copies are byte-identical (`cmp .claude/skills/ryo-ui/SKILL.md src/cli/skills/ryo-ui.md`) and that the CLI's skill tests pass (`cargo test -p ryo2d --lib cli::skills`, or wherever the embedded skills are tested).
- [ ] **Step 2: Record the scrolling review.** For the owner to look at by eye, with the release build of the ui demo before 4i (the merge base, built in a scratch worktree or `git worktree add`, removed after) and after:
  1. Record frames headlessly with `RYO_HEADLESS=1 RYO_DEMO_PAGE=<n> RYO_INPUT=... RYO_CAPTURE_FRAMES=<every frame of a 60-frame gesture>`, for the same wheel gesture and the same trackpad gesture, on pages 10 (Scroll Views), 7 (Clipping & Modals) and 17 (Big list).
  2. Do the same for the settings screen in `benches`' fixture through a small scratch example, or skip it with a note.
  3. Assemble each before-and-after pair into a side-by-side animated GIF (`magick -delay 2 ... -loop 0`) under `/private/tmp/claude-501/4i-review/`.
  4. List the files in your report. The controller shows them to the owner at hand-play.
- [ ] **Step 3: Run** `cargo test --workspace`. Expected: PASS.
- [ ] **Step 4: Commit.**

```bash
git add src/core/ui/AGENTS.md src/core/render/AGENTS.md .claude/skills/ryo-ui/SKILL.md src/cli/skills/ryo-ui.md .claude/skills/ryo-renderer/SKILL.md src/cli/skills/ryo-renderer.md
git commit -m "docs: UI fragments and whole-pixel scroll slots"
```
