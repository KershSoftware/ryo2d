# Fast Big UIs (4e) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every UI frame much cheaper (closure and node UI alike) by cutting the measured per-frame costs, and add fixed-size virtual lists (closure `ui.list` and a pooled node `List`) whose cost does not grow with the item count.

**Architecture:** Part 1 changes the shared builder pipeline in place: one fast word-wise hasher for engine keys, text hashed once per leaf and reused as the text cache key, buffers kept between frames, a layout diff that compares one hash per node, looks without allocation, a cheaper node walk. Part 2 builds lists inside the existing scroll area (spacers around the rows in view, ids keyed by index), shared by the closure builder and a node `List` whose few pooled row entities are bound to items by a `UiListBind` message.

**Tech Stack:** Rust 2024, bevy_ecs 0.20 (only through `src/core/ecs/kernel.rs`), taffy (only in `src/core/ui/layout/`), parley/swash (only in `src/core/text/`), criterion benches, xctrace for the final profile. No new dependencies.

**Spec:** `docs/specs/2026-10-18-ui-fast-design.md`

## Global Constraints

- No `unwrap`, `expect` or `panic!` in library code (tests may use them). Arithmetic on counts, rows and offsets must not overflow or produce non-finite values (use `f64` or saturating math for `rows × stride`).
- LF line endings; 2-space indent (`rustfmt.toml`); comments normal and minimal, in the surrounding code's style.
- Crate boundaries: bevy_ecs only through `src/core/ecs/kernel.rs`; taffy only in `src/core/ui/layout/`; parley/swash/fdsm only in `src/core/text/`. No new dependencies (enabling a feature of an existing dependency, such as serde's `rc`, is allowed).
- Debug-only code carries `#[cfg_attr(not(debug_assertions), allow(dead_code))]`.
- Behaviour does not change in Part 1: every existing test passes, and the demo tour's captures stay pixel-identical (Task 6).
- Warnings: `crate::core::ui::theme::warn_once(key: String, message: impl FnOnce() -> String)`.
- Skills: edit `.claude/skills/<name>/SKILL.md`, then `cp` to `src/cli/skills/<name>.md` and check with `cmp`.
- Commits: `type: brief description` and bullets. **No trailer or attribution line of any kind** (no Co-Authored-By, no Claude-Session). Never mention AGENTS.md in a commit. Stage files by name; never `git add -A`; never add `data/` or `target/`; no bare `git stash`.
- The gate, all green before a task is done:
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`
  - `cargo test --workspace`
  - `cargo check --release -p ryo2d --lib`
- Benches are measured in quiet runs (no other heavy process), `cargo bench --bench ui -- "<filter>"`.

## Rulings Made While Planning

These correct or sharpen the spec; the plan's commit updates the spec to match.

1. **When node list rows are bound.** A scene's `update` runs at the fixed rate (it may not run in a frame at all) and after the UI pass; `ui` runs every frame, after the scene's node trees are drawn. So the pool updates at the start of the UI pass, after this frame's wheel, scroll requests and navigation are resolved and before any node tree is drawn, sized to span the scroll area's current offset and its target (plus the overscan and a focused row). A row given an index in a frame is **drawn from the next frame**, so the game binds it in between: in `ui` (recommended: it runs every frame), in an observer on the row (immediately), or in `update`. A new list's first frame shows no rows.
2. **Focusing an index** applies when that row is built and drawn: in closure UI, the first focusable widget built in the row takes the focus; in node UI, the first focusable node in the row. It can take up to two frames.
3. **The row shape is a component**, not part of the node kind: `UiNode::list(..).row(f)` stores a factory that spawn inserts as `UiListShape`; files store the template. The kind holds only plain settings (row height, columns, overscan).
4. **The item count is state** (like `UiValue`): `ctx.ui_list_count` writes it, and a reload or `update_ui` writes it only when the tree's count changed (4d's three-way rule).
5. **Rows are binding scopes**: `bindings`' root lookup stops at a list row, so `ctx.ui_set(row, ..)` and the patterns inside the row use that row's values.

## Review Focus

1. **A count that shrinks under the focus** (the focused item is past the new end): no panic; the row is released; the focus moves to the new last item if the list had the focus, else is dropped; the scroll offset is clamped. Tests in Task 8 (closure) and Task 10 (node).
2. **Hostile sizes**: `row_height` of 0, negative, NaN or infinite, `count` of `usize::MAX`, `columns` of 0: a warning and no rows (or clamping), never a panic, overflow or non-finite content length. Test in Task 7, and Task 9 for the node kind.
3. **A list inside a modal and inside another scroll area**: the wheel goes to the innermost area that can move (4b's rule), and pad navigation walks the list and leaves it at its ends. Test in Task 8.
4. **Font, scale or theme changes** while text is cached under the new keys: the scale and the font generation are part of the text key, so a change relays out; a cache hit is confirmed against its source. Test in Task 2.
5. **A file reload that changes the row template while a row is focused**: the pooled rows are rebuilt from the new shape and rebound, and the focus stays on the same item. Test in Task 11.

## File Map

| File | Task | Responsibility |
|---|---|---|
| `src/core/hash.rs` | 1 | The shared fast hasher (`FastHasher`, `FastMap`, `FastSet`, `fast_hash`) |
| `src/core/ui/id.rs` | 1 | Ids, now using `core::hash` |
| `src/core/ui/memory.rs`, `src/core/ui/nav.rs`, `src/core/ui/layout/engine.rs`, `src/core/text/engine.rs` | 1-4 | Maps on the fast hasher; scratch buffers; keyed text calls; the hash diff |
| `src/core/text/key.rs` | 2 | `TextKey`: a text's key parts, hashed once |
| `src/core/text/{engine,rich,style}.rs` | 2 | Keyed layout and measure; hit confirmation; colourless style hashing |
| `src/core/ui/immediate/context.rs`, `src/core/ui/tree.rs` | 2, 3 | Text leaves carry their key; subtree and resolve buffers reused |
| `src/core/ui/layout/{engine,mod}.rs` | 4 | `UiStyle` hashing; the diff by hash |
| `src/core/ui/theme/look.rs` and widget call sites | 5 | Skins without allocation |
| `src/core/ui/nodes/{draw,reconcile,spawn,components}.rs`, `src/core/ui/file/document.rs` | 5 | One read per node; cached reload query; build generation |
| `tests/ui_alloc.rs` | 3 | Allocation-counting test (its own test binary) |
| `benches/ui.rs` | 6, 12 | Guard notes; list benches |
| `src/core/ui/widgets/list.rs` | 7, 8 | Closure `ui.list`, `ListState`, jump and focus |
| `src/core/ui/nodes/list.rs` | 9, 10 | The node `List`: pool, `UiListRow`, `UiListBind`, API |
| `src/core/ui/nodes/{node,draw,bindings,mod}.rs` | 9 | `Kind::List`, drawing pooled rows, row binding scope |
| `src/core/scene/context.rs`, `src/core/testing/ui.rs`, `src/core/engine/frame.rs`, `src/core/messages/mod.rs`, `src/lib.rs`, `src/core/ui/mod.rs` | 9, 10 | Wiring and API |
| `src/core/ui/file/vocab.rs`, `src/core/ui/file/build.rs` | 11 | `List` in files |
| `examples/demos/ui/...` | 13 | The Big list page |
| skills, AGENTS.md files, `docs/backlog/ui-retained-nodes.md` | 14, 15 | Docs and the final recommendation |

---

## Part 1: The cheap frame

### Task 1: One fast hasher

**Files:**
- Create: `src/core/hash.rs`
- Modify: `src/core/mod.rs` (`pub(crate) mod hash;` in the foundation section, near `error`), `src/core/ui/id.rs` (use `core::hash`), `src/core/ui/memory.rs` (`prev`, `next`, `states`, `focus_memory` on `FastMap`), `src/core/ui/layout/engine.rs` (`entries`, `index_of`), `src/core/ui/nav.rs` (every `HashMap`/`HashSet` keyed by `UiId`), `src/core/text/engine.rs` (`cache`, `lines`, `coords`, `warned` on `FastMap`/`FastSet`)
- Test: `src/core/hash.rs` (`mod tests`)

**Interfaces:**
- Produces (crate):
  ```rust
  // src/core/hash.rs
  pub(crate) struct FastHasher(u64);            // Default (seeded), Hasher (word-wise, as 4c's IdHasher)
  pub(crate) type FastBuild = BuildHasherDefault<FastHasher>;
  pub(crate) type FastMap<K, V> = HashMap<K, V, FastBuild>;
  pub(crate) type FastSet<K> = HashSet<K, FastBuild>;
  pub(crate) fn fast_hash<T: Hash + ?Sized>(value: &T) -> u64;
  pub(crate) fn fold(a: u64, b: u64) -> u64;    // the folded multiply, for id.rs's mix
  ```
- Consumes: 4c's `IdHasher`, `fold`, `mix`, constants in `src/core/ui/id.rs` (move the hasher and `fold` to `hash.rs`; `mix`, `UiId` and `key_hash` stay in `id.rs`, with `key_hash` now `fast_hash`).

Rules:
- `FastHasher` is 4c's `IdHasher` moved unchanged (same constants, same `write` word-wise loop with the length-tagged tail, same `write_u*`, same `finish`), renamed. `id.rs` keeps producing exactly the same ids (its tests must pass unchanged).
- Maps change type only (`HashMap::new()` becomes `FastMap::default()`); no logic changes.
- Before any code change, capture the demo tour as the pixel baseline for Task 6: run `./dev.sh demos:tour` (or the equivalent the repo's `dev.sh` offers for the tour; check `./dev.sh help`) and copy `target/demo-tour/` to `/private/tmp/claude-501/4e-tour-before/`. Record the path in the report.

- [ ] **Step 1: Capture the pixel baseline** (as above), and run `cargo bench --bench ui` once, recording the numbers in the report as "before".

- [ ] **Step 2: Write the failing tests** in `src/core/hash.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn equal_values_hash_equal_and_maps_work() {
    assert_eq!(fast_hash(&"score"), fast_hash(&"score"));
    let mut map: FastMap<u64, &str> = FastMap::default();
    map.insert(7, "seven");
    assert_eq!(map.get(&7), Some(&"seven"));
  }

  #[test]
  fn similar_strings_do_not_collide() {
    let mut seen = FastSet::default();
    for i in 0..100_000u32 {
      // Labels that differ in one character or one digit, and by length
      let label = format!("Row {i}");
      assert!(seen.insert(fast_hash(&label)), "collision at {label}");
    }
    assert_ne!(fast_hash(&"ab"), fast_hash(&"ab\0"));
    assert_ne!(fast_hash(&("a", "bc")), fast_hash(&("ab", "c")));
  }

  #[test]
  fn small_integers_spread_across_buckets() {
    // A map's bucket index comes from the low and high bits; consecutive ids must not cluster
    let mut low = FastSet::default();
    for i in 0..4096u64 {
      low.insert(fast_hash(&i) & 0xfff);
    }
    assert!(low.len() > 2400, "{} distinct low-12-bit values of 4096", low.len());
  }
}
```

- [ ] **Step 3: Run them to see them fail** — `cargo test -p ryo2d --lib core::hash` fails to compile (no module).

- [ ] **Step 4: Implement**: create `hash.rs` by moving the hasher from `id.rs`; switch the listed maps. Keep the `id.rs` tests passing unchanged.

- [ ] **Step 5: Run the tests** — `cargo test -p ryo2d --lib core::hash core::ui core::text` passes.

- [ ] **Step 6: Bench and commit.** Run `cargo bench --bench ui` and record the numbers in the report (expect the settings frames to fall noticeably). Run the gate, then:

```bash
git add src/core/hash.rs src/core/mod.rs src/core/ui/id.rs src/core/ui/memory.rs src/core/ui/layout/engine.rs src/core/ui/nav.rs src/core/text/engine.rs
git commit -m "perf: one fast hasher for the engine's UI and text keys

- 4c's word-wise id hasher becomes core::hash, shared by the UI and the text engine
- UiMemory, layout, navigation and text cache maps use it instead of SipHash"
```

---

### Task 2: Text hashed once

**Files:**
- Create: `src/core/text/key.rs`
- Modify: `src/core/text/mod.rs`, `src/core/text/engine.rs` (keyed entry points; `Cached` keeps its source; hit confirmation), `src/core/text/style.rs` (`TextStyle::hash_shape_into`), `src/core/ui/immediate/context.rs` (`text_seed` replaced by `TextKey`; `text_leaf` and `UiText::record` pass the key; `content_key` from the key), `src/core/ui/tree.rs` (`TextLeaf` and `DrawKind::Text` carry the `TextKey`; `measure_text` and `command_of` call the keyed engine methods), `src/core/ui/widgets/rebind.rs` (its `text_seed` use)
- Test: `src/core/text/key.rs`, `src/core/text/engine.rs` (`mod tests`), `src/core/ui/immediate/context.rs` (`mod tests`)

**Interfaces:**
- Consumes: Task 1's `FastHasher`, `fast_hash`.
- Produces (crate):
  ```rust
  // src/core/text/key.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub(crate) struct TextKey { text: u64, shape: u64, colours: u64, options: u64 }
  impl TextKey {
    pub(crate) fn of(text: &RichText, style: &TextStyle, options: &TextLayoutOptions) -> TextKey;
    pub(crate) fn seed(&self) -> u64;                       // text + shape + options: colours excluded (was text_seed)
    pub(crate) fn with_options(&self, options: &TextLayoutOptions) -> TextKey;  // re-key wrap/align without rehashing the text
    pub(crate) fn cache_key(&self, scale: f32, generation: u64) -> u64;         // all parts + scale + font generation
  }
  // src/core/text/style.rs
  impl TextStyle { pub(crate) fn hash_shape_into(&self, h: &mut impl Hasher); pub(crate) fn hash_colours_into(&self, h: &mut impl Hasher); }
  // hash_into stays and equals shape then colours
  // src/core/text/engine.rs
  impl TextEngine {
    pub(crate) fn layout_keyed(&mut self, key: TextKey, text: &RichText, style: &TextStyle, options: &TextLayoutOptions, scale: f32) -> Option<Arc<TextLayout>>;
    pub(crate) fn measure_keyed(&mut self, key: TextKey, text: &RichText, style: &TextStyle, options: &TextLayoutOptions, scale: f32) -> Vec2;
    pub(crate) fn min_content_width_keyed(&mut self, key: TextKey, text: &RichText, style: &TextStyle, scale: f32) -> f32;
    // layout / measure / min_content_width stay, now computing TextKey::of and delegating
  }
  ```

Rules:
- `TextKey::of` hashes the text once (`RichText::hash_into` with `FastHasher`), the style's shape fields once, its colour fields once, and the options once; `seed` and `cache_key` combine those parts with `fold`, never rehashing the text. `with_options` replaces the options part (tree resolve lays out with a final wrap width that differs from the measure's).
- `hash_shape_into` hashes every `TextStyle` field `hash_into` hashes except `color` and the colours of `outline`, `shadow` and `glow`; `hash_colours_into` hashes exactly those. This replaces `text_seed`'s style clone (the colours whited out).
- `Cached` stores the request's source (`RichText`, `TextStyle`, `TextLayoutOptions`, `scale`) cloned once on a miss. A hit is used only if the stored source equals the request; otherwise it is a miss and the entry is replaced. `layout_keyed` debug-asserts nothing; it simply confirms.
- Each text leaf in the UI computes its `TextKey` once per frame (in `UiText::record` / `text_leaf` / `push_draw`, wherever the seed was computed), stores it in `TextLeaf`, and every later engine call for that leaf (measure passes, min-content width, the final layout) uses the keyed methods. `content_key` becomes `key.cache_key(px_scale, generation)` (or `fold` of seed, scale and generation, matching the old meaning: content changed).
- The output is unchanged: same layouts, same commands (Task 6's pixel check).

- [ ] **Step 1: Write the failing tests**

In `src/core/text/key.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::text::{RichText, TextLayoutOptions, TextStyle};
  use crate::core::ui::Color;

  #[test]
  fn colours_change_the_cache_key_but_not_the_seed() {
    let text = RichText::from("Score: 10");
    let white = TextStyle::default().size(16.0);
    let red = TextStyle::default().size(16.0).color(Color::new(255, 0, 0, 255));
    let options = TextLayoutOptions::default();
    let a = TextKey::of(&text, &white, &options);
    let b = TextKey::of(&text, &red, &options);
    assert_eq!(a.seed(), b.seed(), "the seed ignores colours");
    assert_ne!(a.cache_key(1.0, 0), b.cache_key(1.0, 0), "the cache key does not");
  }

  #[test]
  fn scale_generation_and_options_change_the_cache_key() {
    let text = RichText::from("Hi");
    let style = TextStyle::default();
    let key = TextKey::of(&text, &style, &TextLayoutOptions::default());
    assert_ne!(key.cache_key(1.0, 0), key.cache_key(2.0, 0));
    assert_ne!(key.cache_key(1.0, 0), key.cache_key(1.0, 1));
    let wrapped = TextLayoutOptions { wrap: Some(100.0), ..TextLayoutOptions::default() };
    assert_eq!(key.with_options(&wrapped), TextKey::of(&text, &style, &wrapped));
  }

  #[test]
  fn shape_and_colour_hashes_together_match_the_full_hash() {
    use crate::core::hash::FastHasher;
    use std::hash::Hasher;
    let style = TextStyle::default().size(20.0).color(Color::new(1, 2, 3, 255));
    let mut full = FastHasher::default();
    style.hash_into(&mut full);
    let mut parts = FastHasher::default();
    style.hash_shape_into(&mut parts);
    style.hash_colours_into(&mut parts);
    assert_eq!(full.finish(), parts.finish());
  }
}
```

In `src/core/text/engine.rs` tests (use the existing test font setup there):

```rust
  #[test]
  fn a_keyed_hit_is_confirmed_against_its_source() {
    let mut engine = test_engine();
    let style = TextStyle::default();
    let options = TextLayoutOptions::default();
    let a = RichText::from("first");
    let b = RichText::from("second");
    let key = TextKey::of(&a, &style, &options);
    let first = engine.layout_keyed(key, &a, &style, &options, 1.0).unwrap();
    // The same key with other text (a forced collision) must not return the first layout
    let second = engine.layout_keyed(key, &b, &style, &options, 1.0).unwrap();
    assert_eq!(first.text, "first");
    assert_eq!(second.text, "second");
  }

  #[test]
  fn a_font_generation_change_relays_out() {
    let mut engine = test_engine();
    let text = RichText::from("Hello");
    let style = TextStyle::default();
    let options = TextLayoutOptions::default();
    let key = TextKey::of(&text, &style, &options);
    let before = engine.layout_keyed(key, &text, &style, &options, 1.0).unwrap();
    bump_font_generation(&mut engine); // the existing tests' way to change fonts (sync with a new Fonts)
    let after = engine.layout_keyed(key, &text, &style, &options, 1.0).unwrap();
    assert!(!Arc::ptr_eq(&before, &after), "a new font generation is a new layout");
  }
```

(Adapt `test_engine` and `bump_font_generation` to the helpers the file's tests already use; if none changes the generation, add a small test helper there.)

In `src/core/ui/immediate/context.rs` tests:

```rust
  #[test]
  fn a_label_is_hashed_once_per_frame() {
    let mut h = crate::core::testing::UiHarness::new(400.0, 300.0);
    h.frame(|ui| { ui.label("Settings"); });
    let before = crate::core::text::key::KEYS_MADE.with(|n| n.get());
    h.frame(|ui| { ui.label("Settings"); });
    let made = crate::core::text::key::KEYS_MADE.with(|n| n.get()) - before;
    assert_eq!(made, 1, "one TextKey for one label in one frame");
  }
```

(`KEYS_MADE` is a `#[cfg(test)] thread_local! { pub(crate) static KEYS_MADE: Cell<u64> }` incremented in `TextKey::of`.)

- [ ] **Step 2: Run them to see them fail** — compile errors (no `TextKey`, no keyed methods).

- [ ] **Step 3: Implement** by the rules.

- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::text core::ui` passes, including every existing text and UI test.

- [ ] **Step 5: Bench and commit.** Record `cargo bench --bench ui` numbers. Run the gate, then:

```bash
git add src/core/text/key.rs src/core/text/mod.rs src/core/text/engine.rs src/core/text/style.rs src/core/ui/immediate/context.rs src/core/ui/tree.rs src/core/ui/widgets/rebind.rs
git commit -m "perf: hash each UI text leaf once and reuse it as the cache key

- TextKey hashes text, style shape, colours and options once; seed and cache key combine the parts
- Every measure and layout of a leaf uses the key instead of rehashing
- A cache hit is confirmed against its stored source, so a collision is a miss, never wrong text"
```

---

### Task 3: Buffers kept between frames

**Files:**
- Modify: `src/core/ui/memory.rs` (a `Scratch` owned by `UiMemory`), `src/core/ui/tree.rs` (`resolve` uses and returns buffers), `src/core/ui/immediate/context.rs` (`push_node` takes a pooled `Subtree`; `finish_subtree` returns buffers), `src/core/ui/popup.rs` (popup subtrees from the pool), `src/core/ui/layout/engine.rs` (`try_layout`'s scratch vectors and index map kept in the engine)
- Create: `tests/ui_alloc.rs` (its own test binary, with a counting global allocator)
- Test: `tests/ui_alloc.rs`

**Interfaces:**
- Consumes: Task 1's `FastMap`.
- Produces (crate): `UiMemory::scratch: Scratch` with `take_subtree(at, clip, scope) -> Subtree` and `give_subtree(Subtree)` (clearing, keeping capacity); `tree::resolve` keeps its per-call vectors in `Scratch` (`descs`, `floating`, `shift`, `exact`, `rects`, `clips`, `content`) and returns what callers read (the first rect) without allocating; `LayoutEngine` keeps `index_of`, `parents`, `visited`, `kids`, `kid_range`, `order`, `child_ids` and `placed` as fields, cleared per call.

Rules:
- No behaviour change. A subtree's node and draw vectors come from the pool and go back to it after `resolve`. Resolve's and the layout's per-call vectors are cleared and refilled, never reallocated once warm.
- The per-node `children: Vec<usize>` of container nodes may still allocate (one small vector per container); leaves must not allocate (`Vec::new()` does not).
- `tests/ui_alloc.rs` installs a counting `#[global_allocator]` (a `System` wrapper with an `AtomicUsize` count of `alloc`/`realloc` calls) and measures allocations of one frame of the 200-widget settings screen after warm-up (copy the `Settings` closure screen from `benches/ui.rs` into the test).

- [ ] **Step 1: Write the failing test** `tests/ui_alloc.rs`:

```rust
//! Allocations per UI frame, counted by a global allocator in this test binary

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use ryo2d::prelude::*;
use ryo2d::testing::UiHarness;

struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    unsafe { System.alloc(layout) }
  }
  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
  }
  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    unsafe { System.realloc(ptr, layout, size) }
  }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

// (paste the bench's Settings struct and its `ui` method here, unchanged)

#[test]
fn an_unchanged_settings_frame_allocates_little() {
  let mut harness = UiHarness::new(640.0, 480.0);
  let mut settings = Settings::new();
  for _ in 0..5 {
    harness.frame(|ui| settings.ui(ui));
  }
  let before = ALLOCATIONS.load(Ordering::Relaxed);
  harness.frame(|ui| settings.ui(ui));
  let made = ALLOCATIONS.load(Ordering::Relaxed) - before;
  println!("allocations in one frame: {made}");
  assert!(made < 200, "an unchanged 200-widget frame made {made} allocations");
}
```

Run it first on the unchanged code to record the "before" count in the report (expect well over a thousand), then keep the threshold at 200 (or lower it to about half the measured "after" count plus headroom once Step 3 is done, and say what you chose).

- [ ] **Step 2: Run it to see it fail** — `cargo test --test ui_alloc -- --nocapture` fails with the before count.

- [ ] **Step 3: Implement** by the rules. Where labels are copied into a new `RichText`/`String` every frame and a borrowed or shared value serves, avoid the copy; report what you changed and what remains.

- [ ] **Step 4: Run it to see it pass**, then the UI tests: `cargo test --test ui_alloc -- --nocapture` and `cargo test -p ryo2d --lib core::ui`.

- [ ] **Step 5: Bench and commit.** Record the bench numbers and the before/after allocation counts. Run the gate, then:

```bash
git add src/core/ui/memory.rs src/core/ui/tree.rs src/core/ui/immediate/context.rs src/core/ui/popup.rs src/core/ui/layout/engine.rs tests/ui_alloc.rs
git commit -m "perf: keep the UI's per-frame buffers between frames

- Subtree, resolve and layout vectors come from scratch kept in UiMemory and the layout engine
- A test counts allocations in one unchanged settings frame"
```

---

### Task 4: A cheaper layout diff

**Files:**
- Modify: `src/core/ui/layout/mod.rs` (`UiStyle::shape_hash`), `src/core/ui/layout/engine.rs` (`Entry` keeps a hash, not a cloned style; sanitize and `to_taffy` only on a change; a restyle counter)
- Test: `src/core/ui/layout/engine.rs` (`mod tests`)

**Interfaces:**
- Consumes: Task 1's `FastHasher`; Task 3's kept vectors.
- Produces (crate): `impl UiStyle { pub(crate) fn shape_hash(&self) -> u64 }` (every field, floats by `to_bits`, `Flow` and its tracks); `LayoutEngine::stats() -> LayoutStats { restyled: u64, rechildren: u64, created: u64 }` (counts since the last `begin_frame`, `#[cfg(test)]` or always; cheap counters).

Rules:
- An `Entry` stores `hash: u64`, the hash of the node's raw style, its parent's raw flow and its measure kind. Each frame the diff computes that hash for each node; when it equals the entry's, nothing is cleaned, converted or sent to taffy for the style (children and `content_key` are diffed as now). When it differs, the style is sanitized and converted (`to_taffy`) and `set_style` is called, as now.
- The sanitized parent flow needed by a changed child is computed then (sanitize the parent's style on demand).
- No behaviour change.

- [ ] **Step 1: Write the failing tests**:

```rust
  #[test]
  fn an_unchanged_tree_restyles_nothing() {
    let mut h = crate::core::testing::UiHarness::new(400.0, 300.0);
    let screen = |ui: &mut crate::core::ui::UiContext<'_>| {
      ui.column().gap(4.0).show(|ui| {
        for i in 0..20 {
          ui.button(format!("B{i}")).id(i).fill_width();
        }
      });
    };
    h.frame(screen);
    h.frame(screen);
    let stats = h.layout_stats();
    assert_eq!(stats.restyled, 0);
    assert_eq!(stats.created, 0);
  }

  #[test]
  fn one_changed_style_restyles_one_node() {
    let mut h = crate::core::testing::UiHarness::new(400.0, 300.0);
    let mut gap = 4.0;
    let mut screen = |ui: &mut crate::core::ui::UiContext<'_>, gap: f32| {
      ui.column().gap(gap).show(|ui| {
        for i in 0..5 {
          ui.button(format!("B{i}")).id(i);
        }
      });
    };
    h.frame(|ui| screen(ui, gap));
    gap = 8.0;
    h.frame(|ui| screen(ui, gap));
    assert_eq!(h.layout_stats().restyled, 1, "only the column's style changed");
  }
```

(Add `UiHarness::layout_stats(&self) -> LayoutStats` as a `#[doc(hidden)]` or crate-visible test accessor; if `UiHarness` cannot expose a crate type publicly, write these tests inside `src/core/ui/layout/engine.rs` driving a `UiContext` the way the existing layout tests do.)

- [ ] **Step 2: Run them to see them fail** (no `stats`).

- [ ] **Step 3: Implement** by the rules.

- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes.

- [ ] **Step 5: Bench and commit.** Record the bench numbers. Run the gate, then:

```bash
git add src/core/ui/layout/mod.rs src/core/ui/layout/engine.rs src/core/testing/ui.rs
git commit -m "perf: diff UI layout nodes by one hash instead of cloning styles

- Unchanged nodes skip sanitizing, conversion and the taffy style update
- Layout stats count restyled, rechildren and created nodes per frame"
```

---

### Task 5: Looks, the node walk and the reload check

**Files:**
- Modify: `src/core/ui/theme/look.rs` (`Look.skin` and `LookPatch.skin` as `Option<Arc<str>>`), `Cargo.toml` (serde's `rc` feature, if used), the widget call sites that read `skin`, `src/core/ui/nodes/draw.rs` (one component read per node), `src/core/ui/nodes/reconcile.rs` (`reload_trees` keeps its query; generations from the source), `src/core/ui/nodes/components.rs` or `UiNodes` (where the cached query lives), `src/core/ui/file/document.rs` (`TreeSource.generation`), `src/core/ui/nodes/spawn.rs` (`UiSource` from the source's generation)
- Test: `src/core/ui/theme/look.rs`, `src/core/ui/nodes/reconcile.rs` (`mod reload_tests`)

**Interfaces:**
- Consumes: 4d's `TreeSource`, `UiSource`, `reload_trees`, `set_source`, `build_source`.
- Produces: `TreeSource { .., pub(crate) generation: u64 }` (the file generation the tree was built from).

Rules:
- **Looks:** `skin` becomes `Option<Arc<str>>` so `look_of` copies a look without a heap allocation; theme files read and write the same RON (serde with the `rc` feature, or a small `serde(with)` module). Every theme test passes unchanged.
- **Node walk:** `draw_node` reads the node's components in one lookup (a tuple through the existing `components::<Q>()` helper, or one `get_components` call) instead of separate `get`/`contains` calls for kind, hidden, label/value, tooltip, disabled and bind. Behaviour is unchanged (every node test passes).
- **Reload query:** `reload_trees` keeps a `QueryState<(Entity, &UiSource)>` between frames (in `UiNodes` or a small resource), created the first time `UiSource` exists; the empty case stays one cheap check.
- **Build generation (4d's deferred defect):** `TreeSource` records `generation` when `build_root`/`build_template`/`build_source` builds it. Spawn's `UiSource`, `set_source` and the reload's bookkeeping use the source's generation, not the file's current one. A tree built before a reload and spawned after it is therefore stale and reloads.

- [ ] **Step 1: Write the failing tests**

In `reconcile.rs` `reload_tests` (uses the existing `harness` and `rewrite` helpers):

```rust
  #[test]
  fn a_tree_built_before_a_reload_and_spawned_after_it_reloads() {
    let (dir, mut h) = harness(&[("late.ui.ron", r#"(root: Column(children: [Text("a")]))"#)]);
    let file = h.assets().load_ui("late.ui.ron").unwrap();
    let tree = file.root(h.assets()).unwrap();
    std::fs::write(dir.path().join("late.ui.ron"), r#"(root: Column(children: [Text("b")]))"#).unwrap();
    h.reload_ui("late.ui.ron").unwrap();
    let root = h.spawn_ui(tree);
    h.frame(|_| {});
    let text = h.world().get::<Children>(root).unwrap()[0];
    assert_eq!(h.world().get::<UiLabel>(text).unwrap().0, "b");
  }
```

In `look.rs` tests:

```rust
  #[test]
  fn a_skinned_look_round_trips_through_a_theme_file() {
    let mut theme = Theme::dark();
    theme.widgets.button.normal.skin = Some("button.png".into());
    let text = theme.to_ron().unwrap();     // use the file's existing save/serialize helper
    let back = Theme::from_ron(&text).unwrap();
    assert_eq!(back.widgets.button.normal.skin.as_deref(), Some("button.png"));
  }
```

(Use the theme module's existing serialize and parse functions; the point is that the RON form is unchanged.)

- [ ] **Step 2: Run them to see them fail** (the reload test fails: the label stays "a"; the look test fails to compile until the type changes, or passes already — then it guards the change).

- [ ] **Step 3: Implement** by the rules.

- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes.

- [ ] **Step 5: Bench and commit.** Record the bench numbers. Run the gate, then:

```bash
git add Cargo.toml src/core/ui/theme/look.rs src/core/ui/nodes/draw.rs src/core/ui/nodes/reconcile.rs src/core/ui/nodes/spawn.rs src/core/ui/nodes/components.rs src/core/ui/file/document.rs
git commit -m "perf: looks without allocation, one read per node, a kept reload query

- Look skins are shared strings, so copying a look allocates nothing
- The node walk reads each node's components in one lookup; reload_trees keeps its query
- A tree records the file generation it was built from, so one built before a reload reloads"
```

(Add the widget files you touched for `skin` to the `git add`.)

---

### Task 6: Measure the cheap frame

**Files:**
- Modify: `benches/ui.rs` (the guard's note), `src/core/ui/AGENTS.md` (the guard and budgets lines), `docs/backlog/ui-retained-nodes.md` (Part 1 numbers)
- Test: the benches and the demo tour

**Interfaces:**
- Consumes: Tasks 1-5.
- Produces: the measured Part 1 numbers; the guard documented as `max(10%, 10 µs)`.

Rules:
- Run `cargo bench --bench ui` in a quiet run (twice; take the second) and compare with the targets: closure settings under 60 µs; node settings within `max(10%, 10 µs)` of the closure one; 1,000-row plain list under 350 µs; empty frame about unchanged (under 250 ns).
- Run the demo tour again and compare every capture with Task 1's baseline (`/private/tmp/claude-501/4e-tour-before/`) using `compare -metric AE <before> <after> null:` (ImageMagick); every page must report 0. A difference is a bug in Part 1: find the task that caused it and fix it before going on.
- If a target is missed, profile (xctrace Time Profiler on the bench, as in the spec) and fix the largest remaining avoidable cost within Part 1's scope; if it still misses after one round, record the numbers and the reason in the report (the controller rules on it).
- Update the guard's wording in `benches/ui.rs` (the doc comment above the settings benches), `src/core/ui/AGENTS.md` (the budgets line: the 200-widget screen budget becomes 60 µs for the closure screen, the node screen within `max(10%, 10 µs)`), and add the Part 1 numbers (before and after, per bench) to the backlog note's 4e section.

- [ ] **Step 1: Bench and compare** with the targets; record the numbers.
- [ ] **Step 2: Run the demo tour and compare** with the baseline; record the results (all 0).
- [ ] **Step 3: Update the docs** as above.
- [ ] **Step 4: Run the gate and commit**:

```bash
git add benches/ui.rs src/core/ui/AGENTS.md docs/backlog/ui-retained-nodes.md
git commit -m "docs: the cheap UI frame's numbers and the new node guard

- The node settings frame may cost the larger of 10% or 10 µs over the closure one
- Before and after numbers for the settings, node and list benches"
```

---
## Part 2: Virtual lists

### Task 7: The closure list

**Files:**
- Create: `src/core/ui/widgets/list.rs`
- Modify: `src/core/ui/widgets/mod.rs`, `src/core/ui/mod.rs` and `src/lib.rs` (export `List`, `ListResponse`), `src/core/ui/widgets/scroll.rs` (crate access the list needs: the scroll's builder options forwarded; reading and setting `ScrollState.target`)
- Test: `src/core/ui/widgets/list.rs` (`mod tests`)

**Interfaces:**
- Consumes: 4b's `Scroll` builder and `ScrollState { offset, target, content, view }`, `UiMemory::{state, peek_state}`, `UiContext::{scroll, column, row}`; Task 1's `fast_hash`.
- Produces:
  ```rust
  // public
  #[must_use = "call .show(|ui, i| ..) to draw the list"]
  pub struct List<'u, 'a> { .. }   // wraps the Scroll builder
  impl<'a> UiContext<'a> {
    #[track_caller] pub fn list<'u>(&'u mut self, count: usize) -> List<'u, 'a>;
    pub fn list_scroll_to(&mut self, list: UiId, index: usize);
  }
  impl List<'_, '_> {
    pub fn row_height(self, height: f32) -> Self;  pub fn columns(self, columns: u16) -> Self;
    pub fn gap(self, gap: f32) -> Self;            pub fn overscan(self, rows: usize) -> Self;
    pub fn id(self, key: impl Hash) -> Self;
    // the scroll area's size and placement options, forwarded: width, height, fill_width, fill_height, fill,
    // min_width, max_width, min_height, max_height, padding, margin, grow, anchor, offset, at, no_bar
    pub fn show(self, row: impl FnMut(&mut UiContext<'_>, usize)) -> ListResponse;
  }
  pub struct ListResponse { pub id: UiId, pub range: Range<usize>, pub response: Response }
  // crate
  pub(crate) struct ListState { pub(crate) stride: f32, pub(crate) rows: usize, pub(crate) columns: u16, pub(crate) count: usize,
    pub(crate) focused: Option<usize>, pub(crate) pending_focus: Option<usize>, pub(crate) focus_ids: Vec<UiId> }   // in UiMemory under the list's id (Task 8 uses the focus fields)
  pub(crate) fn built_rows(rows: usize, stride: f32, view: f32, from: f32, to: f32, overscan: usize) -> Range<usize>;
  ```

Rules (the spec's "Shared shape" and "Closure UI"):
- `stride = row_height + gap`; `rows = count.div_ceil(columns)`; the content length is `rows × stride − gap` (0 for no rows), computed in `f64` and given to layout as `f32`.
- **Built rows:** `built_rows` returns the visual rows covering `[min(offset, target), max(offset, target) + view]` widened by `overscan` rows each side, clamped to `0..rows`. Before the list has a view (its first frame, `view == 0`), the screen's height stands in.
- **Placement:** inside the scroll area (gap 0 between its children is not assumed: use the column gap = `gap`): a leading spacer of height `first × stride − gap` (when `first > 0`), the built rows in order, and a trailing spacer of height `(rows − end) × stride − gap` (when `end < rows`). Spacers record nothing (no hit, focus or hover record). Each built visual row is a container of height `row_height`: with one column, the item's own container `ui.column().id(index).height(row_height).fill_width()`; with several, `ui.row().id(row).height(row_height).gap(gap)` holding one `ui.column().id(index).grow(1.0)` per item and an empty `grow(1.0)` cell for each missing item of the last row, so cells keep equal widths. The row closure builds inside the item's container.
- **Ids follow the index** (the item container is keyed by `index`).
- **Hostile sizes:** a `row_height` that is not finite or not above 0 warns once (`warn_once("ui-list-row-height".into(), ..)`, the message containing "row height") and the list draws as an empty scroll area; `columns(0)` is 1; `overscan` is at least 1; huge counts stay finite (no overflow in `rows × stride`).
- **`list_scroll_to(list, index)`:** reads the `ListState` under `list` and sets the scroll area's `target` by the least movement that shows the item's row (to the row's top if above the view, to its bottom minus the view if below), clamped to the content; an index past the end is clamped to the last item; a list never drawn ignores it.
- `ListResponse.range` is the range of item indices built this frame (`first × columns .. min(end × columns, count)`).

- [ ] **Step 1: Write the failing tests** in `src/core/ui/widgets/list.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::prelude::*;

  fn board(h: &mut UiHarness, count: usize, columns: u16) -> ListResponse {
    h.frame(|ui| {
      ui.list(count).row_height(20.0).gap(2.0).columns(columns).height(200.0).width(300.0)
        .show(|ui, i| { ui.button(format!("Item {i}")).id(i).fill_width(); })
    })
  }

  #[test]
  fn the_built_rows_cover_the_view_and_the_overscan() {
    assert_eq!(built_rows(1000, 22.0, 200.0, 0.0, 0.0, 2), 0..12);       // 200/22 = 9.1 -> 10 rows, + 2
    assert_eq!(built_rows(1000, 22.0, 200.0, 2200.0, 2200.0, 2), 98..112);
    assert_eq!(built_rows(1000, 22.0, 200.0, 21_800.0, 21_800.0, 2), 988..1000);
    assert_eq!(built_rows(1000, 22.0, 200.0, 0.0, 2200.0, 2), 0..112, "spans offset to target");
    assert_eq!(built_rows(0, 22.0, 200.0, 0.0, 0.0, 2), 0..0);
  }

  #[test]
  fn only_the_rows_in_view_are_built_and_rows_sit_at_their_place() {
    let mut h = UiHarness::new(640.0, 480.0);
    let first = board(&mut h, 10_000, 1);
    let built = board(&mut h, 10_000, 1);
    assert!(built.range.end - built.range.start < 20, "{:?}", built.range);
    assert_eq!(built.range.start, 0);
    h.frame(|ui| ui.list_scroll_to(first.id, 5000));
    for _ in 0..60 { board(&mut h, 10_000, 1); }
    let middle = board(&mut h, 10_000, 1);
    assert!(middle.range.contains(&5000), "{:?}", middle.range);
    let row = h.record_of_key(middle.id, 5000).expect("item 5000 is drawn");
    let offset = h.scroll_offset(first.id).y;
    assert!((row.rect.y - (5000.0 * 22.0 - offset)).abs() < 1.0, "row 5000 sits at its place");
  }

  #[test]
  fn the_content_length_is_exact() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 100, 1);
    board(&mut h, 100, 1);
    assert_eq!(h.scroll_content(list.id).y, 100.0 * 22.0 - 2.0);
    let grid = h.frame(|ui| {
      ui.list(10).row_height(20.0).gap(2.0).columns(3).height(200.0).width(300.0).id("grid")
        .show(|ui, i| { ui.button(format!("{i}")).id(i); })
    });
    h.frame(|ui| { ui.list(10).row_height(20.0).gap(2.0).columns(3).height(200.0).width(300.0).id("grid").show(|ui, i| { ui.button(format!("{i}")).id(i); }); });
    assert_eq!(h.scroll_content(grid.id).y, 4.0 * 22.0 - 2.0, "10 items in 3 columns are 4 rows");
  }

  #[test]
  fn an_item_keeps_its_id_wherever_it_is_built() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 1000, 1);
    let id_at = |h: &mut UiHarness| {
      let mut id = None;
      h.frame(|ui| {
        ui.list(1000).row_height(20.0).gap(2.0).height(200.0).width(300.0).show(|ui, i| {
          let r = ui.button(format!("Item {i}")).id(i).show();
          if i == 8 { id = Some(r.id); }
        });
      });
      id
    };
    let before = id_at(&mut h);
    h.frame(|ui| ui.set_scroll_offset(list.id, Vec2::new(0.0, 60.0)));
    let after = id_at(&mut h);
    assert!(before.is_some() && before == after, "item 8 has one id at any scroll position");
  }

  #[test]
  fn hostile_sizes_warn_and_draw_nothing() {
    let mut h = UiHarness::new(640.0, 480.0);
    for height in [0.0, -5.0, f32::NAN, f32::INFINITY] {
      let r = h.frame(|ui| ui.list(10).row_height(height).show(|ui, i| { ui.label(format!("{i}")); }));
      assert_eq!(r.range, 0..0);
    }
    assert_eq!(crate::core::ui::test_util::warnings_containing("row height"), 1);
    let huge = h.frame(|ui| ui.list(usize::MAX).row_height(20.0).height(200.0).show(|ui, i| { ui.label(format!("{i}")); }));
    assert!(huge.range.end > 0);
    h.frame(|ui| { ui.list(usize::MAX).row_height(20.0).height(200.0).show(|ui, i| { ui.label(format!("{i}")); }); });
    assert!(h.scroll_content(huge.id).y.is_finite());
    let zero = h.frame(|ui| ui.list(5).row_height(20.0).columns(0).height(200.0).id("z").show(|ui, i| { ui.label(format!("{i}")); }));
    assert_eq!(zero.range, 0..5, "columns(0) is one column");
  }

  #[test]
  fn an_empty_and_a_short_list_draw_without_scrolling() {
    let mut h = UiHarness::new(640.0, 480.0);
    let empty = h.frame(|ui| ui.list(0).row_height(20.0).height(200.0).id("e").show(|_, _| {}));
    assert_eq!(empty.range, 0..0);
    let short = h.frame(|ui| ui.list(3).row_height(20.0).height(200.0).id("s").show(|ui, i| { ui.label(format!("{i}")); }));
    assert_eq!(short.range, 0..3);
  }

  #[test]
  fn list_scroll_to_moves_by_the_least_amount() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 1000, 1);
    board(&mut h, 1000, 1);
    h.frame(|ui| ui.list_scroll_to(list.id, 100));
    for _ in 0..60 { board(&mut h, 1000, 1); }
    let offset = h.scroll_offset(list.id).y;
    assert!((offset - (101.0 * 22.0 - 2.0 - 200.0)).abs() < 1.0, "row 100 at the bottom: {offset}");
    h.frame(|ui| ui.list_scroll_to(list.id, 5_000_000));
    for _ in 0..60 { board(&mut h, 1000, 1); }
    let end = board(&mut h, 1000, 1);
    assert_eq!(end.range.end, 1000, "past the end clamps to the last item");
  }
}
```

(Add to `UiHarness` the small read helpers the tests use, if missing, as crate-visible test accessors: `scroll_offset(id) -> Vec2` (exists on `UiContext`; expose through the harness), `scroll_content(id) -> Vec2` (the `ScrollState.content` of the last frame), `record_of_key(parent: UiId, key: usize) -> Option<Record>` (the record of the item container keyed `key` under the list: compute the child id the way `IdScopes` makes a keyed id, or record the item container ids into `ListState` for tests).)

- [ ] **Step 2: Run them to see them fail** — compile errors.

- [ ] **Step 3: Implement** by the rules. Inside the scroll area's `show` closure the current id scope's top is the scroll area's id: read the `ScrollState` there (`peek_state`), write `ListState` under the same id.

- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui::widgets::list core::ui` passes.

- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/widgets/list.rs src/core/ui/widgets/mod.rs src/core/ui/widgets/scroll.rs src/core/ui/mod.rs src/lib.rs src/core/testing/ui.rs
git commit -m "feat: ui.list builds only the rows of a long list that are in view

- A scroll area of exact length with spacers around the built rows; grids by columns
- Item ids follow the index; list_scroll_to jumps to any item"
```

---

### Task 8: Closure list focus

**Files:**
- Modify: `src/core/ui/widgets/list.rs` (focus tracking, the focused row kept built, `list_focus`, count shrink), `src/core/ui/id.rs` (a capture of ids made while building a row)
- Test: `src/core/ui/widgets/list.rs` (`mod focus_tests`)

**Interfaces:**
- Consumes: Task 7's `List`, `ListState`, `built_rows`; 4b's `request_focus`, `memory.interact.focused`, `memory.prev(id)` records and their `FOCUSABLE` flag.
- Produces: `impl UiContext { pub fn list_focus(&mut self, list: UiId, index: usize) }`; `IdScopes::{start_capture, take_capture}` (crate).

Rules (the spec's focus bullets and Rulings 1-2):
- **Which row holds the focus:** while building each item, the list checks whether the id that had the focus last frame (`memory.interact.focused`) is made inside that item (compare the set of ids made before and after the item: `IdScopes`' `used` set, or the capture). The item that made it is `ListState.focused`.
- **The focused item stays built:** the built set is the built rows plus the focused item's row (from last frame's `ListState.focused`), with spacers filling the gaps between non-contiguous runs, so the focused row keeps its true place and is clipped when out of view.
- **Count shrink:** if `ListState.focused` is at or past the new count, it is cleared; if the count is above 0 and the list held the focus, `pending_focus` becomes the new last item (Review Focus 1).
- **`list_focus(list, index)`:** scrolls to the item (as `list_scroll_to`) and sets `pending_focus`. When the item's row is next built, the ids made while building it are captured into `ListState.focus_ids`; on the next frame the list requests focus (`ui.request_focus`) for the first of those ids whose last-frame record is focusable, and clears the pending state. It can take two frames.
- **Pad walking** needs nothing new: overscan rows are built, so moving the focus onto the row past the edge works and 4b's scroll-into-view follows.

- [ ] **Step 1: Write the failing tests**:

```rust
#[cfg(test)]
mod focus_tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::prelude::*;

  fn scope_list(h: &mut UiHarness, count: usize) -> ListResponse {
    h.frame(|ui| {
      let mut out = None;
      ui.column().focus_scope().show(|ui| {
        out = Some(ui.list(count).row_height(20.0).height(100.0).width(200.0).id("l").show(|ui, i| {
          let b = ui.button(format!("Item {i}")).id(i);
          if i == 0 { b.default_focus().show(); } else { b.show(); }
        }));
      });
      out.unwrap()
    })
  }

  fn focused_label(h: &UiHarness) -> Option<usize> {
    // The harness exposes the focused id; map it back to an index through ListState.focused
    h.list_state(h.last_list_id()?).and_then(|s| s.focused)
  }

  #[test]
  fn the_pad_walks_to_the_end_of_a_long_list() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = scope_list(&mut h, 300);
    scope_list(&mut h, 300);
    for _ in 0..299 {
      h.action("ui.down");
      scope_list(&mut h, 300);
      scope_list(&mut h, 300);
    }
    assert_eq!(h.list_state(list.id).unwrap().focused, Some(299));
    let max = h.scroll_content(list.id).y - 100.0;
    for _ in 0..60 { scope_list(&mut h, 300); }
    assert!((h.scroll_offset(list.id).y - max).abs() < 1.0, "scrolled to the end");
  }

  #[test]
  fn a_focused_row_scrolled_away_stays_built_and_keeps_the_focus() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = scope_list(&mut h, 300);
    scope_list(&mut h, 300);
    let focused = h.focused();
    assert!(focused.is_some());
    h.frame(|ui| ui.set_scroll_offset(list.id, Vec2::new(0.0, 3000.0)));
    for _ in 0..3 { scope_list(&mut h, 300); }
    assert_eq!(h.focused(), focused, "item 0 is still built, so it keeps the focus");
    h.action("ui.down");
    scope_list(&mut h, 300);
    scope_list(&mut h, 300);
    assert_eq!(h.list_state(list.id).unwrap().focused, Some(1), "navigation resumes from item 0");
  }

  #[test]
  fn list_focus_jumps_to_an_item_and_focuses_it() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = scope_list(&mut h, 300);
    scope_list(&mut h, 300);
    h.frame(|ui| ui.list_focus(list.id, 250));
    for _ in 0..4 { scope_list(&mut h, 300); }
    assert_eq!(h.list_state(list.id).unwrap().focused, Some(250));
  }

  #[test]
  fn a_count_that_shrinks_under_the_focus_moves_it_to_the_last_item() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = scope_list(&mut h, 300);
    scope_list(&mut h, 300);
    h.frame(|ui| ui.list_focus(list.id, 250));
    for _ in 0..4 { scope_list(&mut h, 300); }
    for _ in 0..4 { scope_list(&mut h, 100); }
    assert_eq!(h.list_state(list.id).unwrap().focused, Some(99));
  }

  #[test]
  fn a_list_in_a_modal_inside_a_scroll_area_takes_the_wheel_and_the_pad() {
    let mut h = UiHarness::new(640.0, 480.0);
    let draw = |h: &mut UiHarness| h.frame(|ui| {
      let mut out = None;
      ui.scroll().height(300.0).width(400.0).id("outer").show(|ui| {
        ui.modal().show(|ui| {
          out = Some(ui.list(500).row_height(20.0).height(100.0).width(200.0).id("inner").show(|ui, i| {
            let b = ui.button(format!("{i}")).id(i);
            if i == 0 { b.default_focus().show(); } else { b.show(); }
          }));
        });
      });
      out.unwrap()
    });
    let list = draw(&mut h);
    draw(&mut h);
    let rect = h.record_of(list.id).unwrap().rect;
    h.move_to(rect.x + 10.0, rect.y + 10.0);
    h.wheel(0.0, -3.0);
    for _ in 0..30 { draw(&mut h); }
    assert!(h.scroll_offset(list.id).y > 0.0, "the inner list took the wheel");
    for _ in 0..10 { h.action("ui.down"); draw(&mut h); draw(&mut h); }
    assert_eq!(h.list_state(list.id).unwrap().focused, Some(10));
  }
}
```

(Add crate-visible harness accessors `list_state(id) -> Option<ListState>` (clone of the state in memory), `last_list_id()` if useful, `record_of(id)`; adapt `focused_label` or drop it if unused.)

- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement** by the rules.
- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes.
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/widgets/list.rs src/core/ui/id.rs src/core/testing/ui.rs
git commit -m "feat: closure lists keep the focus on their item

- The focused item's row stays built when scrolled away; the pad walks the whole list
- list_focus jumps to an item and focuses it; a shrinking count moves the focus to the last item"
```

---

### Task 9: The pooled node `List`

**Files:**
- Create: `src/core/ui/nodes/list.rs`
- Modify: `src/core/ui/nodes/node.rs` (`Kind::List(ListOpts)`, `UiNode::list`, `.columns`, `.overscan`, `.row`, `NodeState.list_count`, `Kind::name`, `takes`), `src/core/ui/nodes/draw.rs` (the `Kind::List` arm), `src/core/ui/nodes/bindings.rs` (binding root stops at a list row), `src/core/ui/nodes/spawn.rs` (insert `UiList`/`UiListShape` at spawn; a crate helper spawning a pooled row under a list), `src/core/ui/nodes/reconcile.rs` (`UiApplied` three-way for the count; rows never matched), `src/core/ui/nodes/events.rs` (`UiListBind`), `src/core/ui/nodes/mod.rs`, `src/core/messages/mod.rs`, `src/core/engine/frame.rs` (call `update_lists` in `draw_ui`), `src/core/testing/ui.rs` (the same call; register `UiListBind`), `src/core/ui/mod.rs`, `src/lib.rs` (prelude `UiListBind`, `UiListRow`)
- Test: `src/core/ui/nodes/list.rs` (`mod tests`)

**Interfaces:**
- Consumes: Task 7's `built_rows`, the `List` closure builder's placement code (share it: the node arm draws through the same helper the closure builder uses, so both place rows identically); 4c's spawn, `finish`, `draw_node`, `UiInteraction`; 4d's bindings (`apply_subtree`), reconcile (`UiAdded`, `UiApplied`).
- Produces:
  ```rust
  // public
  impl UiNode {
    pub fn list(count: usize, row_height: f32) -> UiNode;
    pub fn columns(self, columns: u16) -> UiNode;  pub fn overscan(self, rows: u16) -> UiNode;
    pub fn row(self, shape: impl Fn() -> UiNode + Send + Sync + 'static) -> UiNode;
  }
  #[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
  pub struct UiListRow { pub(crate) index: usize, pub(crate) bound_frame: u64 }
  impl UiListRow { pub fn index(&self) -> usize }
  #[derive(Message, EntityEvent, Debug, Clone, Copy, PartialEq, Eq)]
  pub struct UiListBind { pub list: Entity, #[event_target] pub row: Entity, pub index: usize }
  // crate
  #[derive(Debug, Clone, PartialEq)] pub(crate) struct ListOpts { pub(crate) row_height: f32, pub(crate) columns: u16, pub(crate) overscan: u16 }
  #[derive(Clone)] pub(crate) enum RowShape { Code(Arc<dyn Fn() -> UiNode + Send + Sync>) }   // Task 11 adds File
  #[derive(Component)] pub(crate) struct UiListShape(pub(crate) RowShape);
  #[derive(Component, Debug, Default)] pub(crate) struct UiList { pub(crate) count: usize, pub(crate) rows: Vec<(usize, Entity)>,
    pub(crate) focused: Option<usize>, pub(crate) jump: Option<(usize, bool)> }
  pub(crate) fn update_lists(world: &mut World, memory: &mut UiMemory, frame: u64, screen: Vec2);
  ```
  `UiNode` gains `pub(crate) row_shape: Option<RowShape>`.

Rules (the spec's "Node trees: the pooled `List`", and Rulings 1, 3, 4, 5):
- **The kind:** `Kind::List(ListOpts)`, name `"list"`, not a container (`spawn_ui_in` refuses it: its rows are the pool's). `takes`: the scroll area's size and placement options, `padding`, `margin`, `gap`, `grow`, `no_bar`, `columns`, `overscan`, `row`, `embedded`, plus the universal ones. A list without a row shape warns once at spawn and builds no rows. `row_height` hostile values: as Task 7 (warn once, no rows).
- **Spawn:** inserts `UiList { count, .. }` and `UiListShape` (when given); the count goes through `NodeState.list_count` so `UiApplied` gets it (three-way: a reconcile writes `UiList.count` only when the tree's count changed).
- **`update_lists`** runs once a frame (UI pass 0) after the pointer, wheel, scroll requests and navigation are resolved and before any node tree is drawn: in `Engine::draw_ui` right after `resolve_navigation`, and at the same point in `UiHarness::frame_with_world`. For each `UiList`:
  - its scroll state comes from the list entity's last drawn id (`UiInteraction::id`); before the first draw, the screen height stands in for the view;
  - the needed items are those of `built_rows(..)` (spanning offset and target) plus `focused` plus a pending `jump` index, within `0..count`;
  - rows whose index is still needed keep their entity; the others are reused for needed indices without a row; when short, new rows are spawned from the shape as children of the list (with `UiListRow`, `UiAdded` and the row's own binding scope); extra rows are despawned;
  - every row given an index this frame (new or reused) gets `bound_frame = frame` and a `UiListBind { list, row, index }` is written as a message and triggered as an entity event on the row;
  - a pending `jump` sets the scroll area's target (least movement, as Task 7) and is cleared once its row exists (focus is Task 10).
- **Drawing** (`Kind::List` arm): the list draws as the closure list does (same placement helper: spacers and item containers keyed by index), and for each row of `UiList.rows` in index order draws the row entity with `draw_node` inside its item container **only if `bound_frame < frame`** (a row bound this frame shows next frame); otherwise its item container is drawn empty at full size. After drawing each row, if any node in its subtree reports focus (`UiInteraction::focused`), `UiList.focused = Some(index)`.
- **Binding scope:** `bindings`' root lookup (used by `set`, `get`, `sync_root`, `apply_subtree`) stops at the nearest ancestor that is a `UiRoot` **or** a `UiListRow`; `ctx.ui_set(row, ..)` fills only that row.
- **Reconcile:** a list's rows are never matched against a tree's children (they carry `UiAdded`); `update_ui` on a list or its reload updates its settings, keeps its rows, and (when the count changed in the tree) writes the count. The row shape changing is Task 11.
- **Despawn:** despawning a list despawns its rows (they are children).

- [ ] **Step 1: Write the failing tests** in `src/core/ui/nodes/list.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  fn row() -> UiNode {
    UiNode::row().child(UiNode::text("{rank}. {name}").bound().name("line"))
  }

  fn board(h: &mut UiHarness, count: usize) -> Entity {
    h.spawn_ui(UiNode::list(count, 20.0).gap(2.0).height(200.0).width(300.0).row(row).name("board"))
  }

  /// Bind every row the frame asked for, as a game would in its `ui`
  fn bind(h: &mut UiHarness) -> usize {
    let binds: Vec<UiListBind> = h.messages::<UiListBind>().collect();
    for b in &binds {
      h.ui_set(b.row, "rank", b.index + 1);
      h.ui_set(b.row, "name", format!("Player {}", b.index + 1));
    }
    binds.len()
  }

  fn line_of(h: &UiHarness, row: Entity) -> String {
    let line = crate::core::ui::nodes::find_ui_in(h.world(), row, "line").unwrap();
    h.world().get::<UiLabel>(line).map(|l| l.0.clone()).unwrap_or_default()
  }

  fn rows(h: &UiHarness, list: Entity) -> Vec<(usize, Entity)> {
    h.world().get::<UiList>(list).unwrap().rows.clone()
  }

  #[test]
  fn a_pool_holds_about_a_screenful_and_binds_each_row() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10_000);
    h.frame(|_| {});
    let first = bind(&mut h);
    assert!(first > 0 && first < 40, "{first} rows for a 200-unit view (screen height before the first draw)");
    h.frame(|_| {});
    bind(&mut h);
    h.frame(|_| {});
    let pool = rows(&h, list);
    assert!(pool.len() < 20, "{} rows pooled", pool.len());
    let (index, row) = pool[0];
    assert_eq!(line_of(&h, row), format!("{}. Player {}", index + 1, index + 1));
  }

  #[test]
  fn scrolling_reuses_rows_and_binds_them_for_their_new_items() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10_000);
    for _ in 0..3 { h.frame(|_| {}); bind(&mut h); }
    let before: Vec<Entity> = rows(&h, list).iter().map(|r| r.1).collect();
    let id = h.ui_id(list).unwrap();
    h.frame(|ui| ui.set_scroll_offset(id, Vec2::new(0.0, 22.0 * 50.0)));
    let rebound = bind(&mut h);
    assert!(rebound > 0);
    for _ in 0..2 { h.frame(|_| {}); bind(&mut h); }
    let after = rows(&h, list);
    assert!(after.iter().any(|(i, _)| *i == 52));
    assert!(after.iter().all(|(_, e)| before.contains(e)) || after.len() <= before.len() + 2, "rows are reused, not respawned");
    let (_, row) = *after.iter().find(|(i, _)| *i == 52).unwrap();
    assert_eq!(line_of(&h, row), "53. Player 53");
  }

  #[test]
  fn a_row_bound_this_frame_is_drawn_from_the_next() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 100);
    h.frame(|_| {});
    let (_, row) = rows(&h, list)[0];
    let line = crate::core::ui::nodes::find_ui_in(h.world(), row, "line").unwrap();
    assert!(h.ui_id(line).is_none(), "not drawn in the frame it was bound");
    bind(&mut h);
    h.frame(|_| {});
    assert!(h.ui_id(line).is_some(), "drawn the next frame");
  }

  #[test]
  fn each_row_is_its_own_binding_scope() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10);
    h.frame(|_| {});
    bind(&mut h);
    let pool = rows(&h, list);
    assert_ne!(line_of(&h, pool[0].1), line_of(&h, pool[1].1));
  }

  #[test]
  fn hostile_sizes_and_a_missing_shape_build_no_rows() {
    let mut h = UiHarness::new(640.0, 480.0);
    let bad = h.spawn_ui(UiNode::list(10, f32::NAN).row(row));
    let shapeless = h.spawn_ui(UiNode::list(10, 20.0));
    h.frame(|_| {});
    assert!(rows(&h, bad).is_empty());
    assert!(rows(&h, shapeless).is_empty());
    assert_eq!(crate::core::ui::test_util::warnings_containing("row shape"), 1);
  }

  #[test]
  fn a_despawned_list_takes_its_rows_and_spawn_ui_in_refuses_it() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 10);
    h.frame(|_| {});
    let pool = rows(&h, list);
    assert!(spawn_ui_in(h.world_mut(), list, UiNode::text("x")).is_none());
    h.world_mut().despawn(list);
    assert!(pool.iter().all(|(_, e)| h.world().get_entity(*e).is_err()));
  }

  #[test]
  fn a_reconcile_keeps_the_rows_and_writes_the_count_only_when_the_tree_changed_it() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = board(&mut h, 100);
    h.frame(|_| {});
    let pool = rows(&h, list);
    h.world_mut().get_mut::<UiList>(list).unwrap().count = 500;   // the game's count
    h.update_ui(list, UiNode::list(100, 20.0).gap(4.0).height(200.0).width(300.0).row(row).name("board"));
    assert_eq!(h.world().get::<UiList>(list).unwrap().count, 500, "the tree did not change the count");
    assert_eq!(rows(&h, list), pool, "rows are the pool's");
    h.update_ui(list, UiNode::list(42, 20.0).gap(4.0).height(200.0).width(300.0).row(row).name("board"));
    assert_eq!(h.world().get::<UiList>(list).unwrap().count, 42);
  }
}
```

(`h.messages::<UiListBind>()` is the harness's message read from 4d; register `UiListBind` in `UiHarness::new` and in `register_engine_messages`.)

- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement** by the rules.
- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes, and `cargo test --test e2e ui_` (the node and file e2e tests) passes.
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/nodes/list.rs src/core/ui/nodes/node.rs src/core/ui/nodes/draw.rs src/core/ui/nodes/bindings.rs src/core/ui/nodes/spawn.rs src/core/ui/nodes/reconcile.rs src/core/ui/nodes/events.rs src/core/ui/nodes/mod.rs src/core/messages/mod.rs src/core/engine/frame.rs src/core/testing/ui.rs src/core/ui/mod.rs src/lib.rs src/core/ui/widgets/list.rs
git commit -m "feat: a pooled node List bound by UiListBind

- A screenful of row entities reused by index as the list scrolls
- UiListBind for every row given an item; rows draw from the frame after they are bound
- Each row is its own binding scope; rows are the pool's, never a tree's children"
```

---

### Task 10: The node list API and focus

**Files:**
- Modify: `src/core/ui/nodes/list.rs` (API functions; focus kept; jump with focus; count changes), `src/core/scene/context.rs`, `src/core/testing/ui.rs`
- Test: `src/core/ui/nodes/list.rs` (`mod api_tests`)

**Interfaces:**
- Consumes: Task 9's `UiList`, `UiListRow`, `update_lists`, `UiListBind`; 4c's `focus_ui` (`UiNodes::pending_focus`, which waits until the node is drawn).
- Produces:
  ```rust
  impl SceneContext<'_> {   // and the same methods on UiHarness
    pub fn ui_list_count(&mut self, list: Entity, count: usize);
    pub fn ui_list_refresh(&mut self, list: Entity);
    pub fn ui_list_refresh_row(&mut self, list: Entity, index: usize);
    pub fn ui_list_scroll_to(&mut self, list: Entity, index: usize);
    pub fn ui_list_focus(&mut self, list: Entity, index: usize);
    pub fn ui_list_index(&self, node: Entity) -> Option<usize>;
  }
  ```

Rules:
- `ui_list_count` writes `UiList.count`; rows past the end are released at the next `update_lists`; if `focused` is at or past the new count and the count is above 0, the focus moves to the new last item (a pending focus jump), else it is dropped (Review Focus 1); the scroll is clamped by the scroll area as usual.
- `ui_list_refresh` marks every pooled row for rebinding (add a field to `UiList` for pending rebinds, e.g. `rebind: Rebind { None, All, Rows(Vec<usize>) }`): at the next `update_lists` each gets `bound_frame = frame` and a `UiListBind` (so it draws its new data the frame after); `ui_list_refresh_row` does it for the row holding `index`, if pooled.
- `ui_list_scroll_to` sets `UiList.jump = Some((index, false))`; `ui_list_focus` sets `Some((index, true))`. A focus jump, once the row exists and has been drawn bound, gives the focus to the row's first focusable node (depth first: a node whose kind is a focusable widget, not hidden or disabled) through 4c's `pending_focus`.
- `ui_list_index(node)` walks up from `node` to the nearest `UiListRow` and returns its index.
- The focused row stays pooled: `update_lists` always keeps the row of `UiList.focused`.
- Pad walking needs nothing new (overscan rows are pooled, bound and drawn).
- Calls on an entity that is not a list warn once and do nothing.

- [ ] **Step 1: Write the failing tests**:

```rust
#[cfg(test)]
mod api_tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  fn row() -> UiNode {
    UiNode::button("{name}").bound().name("pick")
  }

  fn setup(h: &mut UiHarness, count: usize) -> Entity {
    let root = h.spawn_ui(
      UiNode::column().focus_scope()
        .child(UiNode::list(count, 20.0).height(100.0).width(200.0).row(row).name("list")),
    );
    h.find_ui("list").unwrap_or(root)
  }

  fn step(h: &mut UiHarness) {
    h.frame(|_| {});
    let binds: Vec<UiListBind> = h.messages::<UiListBind>().collect();
    for b in binds { h.ui_set(b.row, "name", format!("P{}", b.index)); }
  }

  fn focused_index(h: &UiHarness, list: Entity) -> Option<usize> {
    h.world().get::<UiList>(list).and_then(|l| l.focused)
  }

  #[test]
  fn ui_list_focus_jumps_and_focuses_and_ui_list_index_maps_events() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 1000);
    for _ in 0..3 { step(&mut h); }
    h.ui_list_focus(list, 700);
    for _ in 0..5 { step(&mut h); }
    assert_eq!(focused_index(&h, list), Some(700));
    h.action("ui.confirm");
    step(&mut h);
    let clicked = h.events().iter().find(|e| e.kind == UiEventKind::Clicked).expect("a click");
    assert_eq!(h.ui_list_index(clicked.node), Some(700));
  }

  #[test]
  fn the_pad_walks_the_list_and_the_focused_row_stays_pooled() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 200);
    for _ in 0..3 { step(&mut h); }
    h.ui_list_focus(list, 0);
    for _ in 0..4 { step(&mut h); }
    for _ in 0..30 { h.action("ui.down"); step(&mut h); step(&mut h); }
    assert_eq!(focused_index(&h, list), Some(30));
    let id = h.ui_id(list).unwrap();
    h.frame(|ui| ui.set_scroll_offset(id, Vec2::ZERO));
    for _ in 0..3 { step(&mut h); }
    assert!(h.world().get::<UiList>(list).unwrap().rows.iter().any(|(i, _)| *i == 30), "the focused row stays pooled");
    assert_eq!(focused_index(&h, list), Some(30));
  }

  #[test]
  fn a_shrinking_count_moves_the_focus_to_the_last_item() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 1000);
    for _ in 0..3 { step(&mut h); }
    h.ui_list_focus(list, 900);
    for _ in 0..5 { step(&mut h); }
    h.ui_list_count(list, 50);
    for _ in 0..6 { step(&mut h); }
    assert_eq!(focused_index(&h, list), Some(49));
    assert!(h.world().get::<UiList>(list).unwrap().rows.iter().all(|(i, _)| *i < 50));
  }

  #[test]
  fn refresh_rebinds_rows() {
    let mut h = UiHarness::new(640.0, 480.0);
    let list = setup(&mut h, 10);
    for _ in 0..3 { step(&mut h); }
    h.ui_list_refresh(list);
    h.frame(|_| {});
    assert_eq!(h.messages::<UiListBind>().count(), h.world().get::<UiList>(list).unwrap().rows.len());
    h.ui_list_refresh_row(list, 3);
    h.frame(|_| {});
    let again: Vec<UiListBind> = h.messages::<UiListBind>().collect();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].index, 3);
  }

  #[test]
  fn list_calls_on_a_plain_node_warn_once() {
    let mut h = UiHarness::new(640.0, 480.0);
    let plain = h.spawn_ui(UiNode::column());
    h.ui_list_count(plain, 3);
    h.ui_list_scroll_to(plain, 3);
    assert_eq!(crate::core::ui::test_util::warnings_containing("is not a list"), 1);
  }
}
```

- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement** by the rules.
- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes.
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/nodes/list.rs src/core/scene/context.rs src/core/testing/ui.rs
git commit -m "feat: drive node lists from game code

- ui_list_count, refresh, refresh_row, scroll_to and focus; ui_list_index maps a UiEvent to its item
- The focused row stays pooled; a shrinking count moves the focus to the last item"
```

---

### Task 11: `List` in UI files

**Files:**
- Modify: `src/core/ui/file/vocab.rs` (the `List` kind and its options), `src/core/ui/file/build.rs` (the `row` value), `src/core/ui/nodes/list.rs` (`RowShape::File`; rows built from the file through the world's `AssetManager`; a changed shape rebuilds and rebinds the pool), `src/core/ui/nodes/reconcile.rs` (a list's shape change on reconcile and reload)
- Test: `src/core/ui/file/build.rs` (vocabulary), `src/core/ui/nodes/list.rs` (`mod file_tests`), `src/core/ui/file/check.rs` (a `List` file checks)

**Interfaces:**
- Consumes: 4d's vocabulary (`KindSpec`, `ArgSpec`, `option!`), `UiFile::build_template` and value building, `UiArgs`, 4d's reload (`reload_trees`, `Mode::Reload`); Task 9's `RowShape`, `UiListShape`, `update_lists`; Task 10's API.
- Produces: `RowShape::File { file: UiFile, row: FileRow }` (a template name with no arguments, or a node value, built in its file's scope); `List` in the vocabulary.

Rules:
- **Vocabulary:** `List` with constructor options `count` (whole, default 0), **`row_height`** (number) and **`row`** (a template name, as a bare CamelCase name or string, or a node value), both required; options `columns` (whole), `overscan` (whole), `gap`, the scroll area's size and placement options, `no_bar`, `embedded` and the universal ones. A missing `row` or `row_height` is `List needs "row"` / `List needs "row_height"`.
- **Building rows:** `update_lists` builds a file row with the world's `AssetManager` (as 4d's reload does), each pooled row a fresh instance of the row shape (each its own binding scope, so the template's `{name}` patterns read the row's values).
- **A changed shape** (the list's tree gives a new `row`, or the row's file reloads): every pooled row is brought in line with the new shape in place (4d's `update_ui` on the row, so entities and focus stay) and marked for rebinding (`bound_frame = frame`, a `UiListBind` for each), so the game refills them. A file row that fails to build keeps the old rows and warns once (as 4d's reload does).
- `ryo2d check-ui` checks `List` with no extra work (it is in the vocabulary); add a check test.

- [ ] **Step 1: Write the failing tests**

In `build.rs` tests: `same("List(count: 5, row_height: 20, gap: 2, columns: 2, row: Text(\"x\"))", ..)` cannot compare a factory; instead assert on the built node's kind and options, and that `List(count: 5)` reports `List needs "row_height"` and `List(row_height: 20)` reports `List needs "row"`:

```rust
  #[test]
  fn a_list_builds_with_its_row_and_needs_both() {
    let (node, issues) = build("List(count: 5, row_height: 20, gap: 2, columns: 2, row: Text(\"x\"))");
    assert!(issues.is_empty(), "{issues:?}");
    let node = node.unwrap();
    assert_eq!(node.kind.name(), "list");
    assert!(node.row_shape.is_some());
    assert_eq!(messages("List(count: 5, row: Text(\"x\"))"), ["List needs \"row_height\""]);
    assert_eq!(messages("List(count: 5, row_height: 20)"), ["List needs \"row\""]);
  }
```

In `list.rs` `file_tests` (with 4d's `harness`/`rewrite` helpers from `reconcile.rs` tests, or local copies):

```rust
  const BOARD: &str = r#"(
    templates: { "LeaderRow": (node: Row(children: [Text(text: "{rank}. {name}", name: "line")])) },
    root: Column(focus_scope: true, children: [List(count: 1000, row_height: 20, height: 100, width: 200, name: "board", row: LeaderRow)]),
  )"#;

  #[test]
  fn a_file_list_pools_rows_from_its_template() {
    let (_dir, mut h) = harness(&[("board.ui.ron", BOARD)]);
    h.spawn_ui_file("board.ui.ron").unwrap();
    let list = h.find_ui("board").unwrap();
    for _ in 0..3 { step(&mut h); }
    let pool = h.world().get::<UiList>(list).unwrap().rows.clone();
    assert!(!pool.is_empty() && pool.len() < 20);
  }

  #[test]
  fn a_reload_of_the_row_template_rebuilds_rows_and_keeps_the_focused_item() {
    let (dir, mut h) = harness(&[("board.ui.ron", BOARD)]);
    h.spawn_ui_file("board.ui.ron").unwrap();
    let list = h.find_ui("board").unwrap();
    for _ in 0..3 { step(&mut h); }
    h.ui_list_focus(list, 40);
    for _ in 0..5 { step(&mut h); }
    assert_eq!(h.world().get::<UiList>(list).unwrap().focused, Some(40));
    rewrite(&dir, &mut h, "board.ui.ron", &BOARD.replace("\"{rank}. {name}\"", "\"#{rank} {name}\""));
    for _ in 0..3 { step(&mut h); }
    let (_, row) = *h.world().get::<UiList>(list).unwrap().rows.iter().find(|(i, _)| *i == 40).unwrap();
    let line = crate::core::ui::nodes::find_ui_in(h.world(), row, "line").unwrap();
    assert_eq!(h.world().get::<UiLabel>(line).unwrap().0, "#41 P40");
    assert_eq!(h.world().get::<UiList>(list).unwrap().focused, Some(40), "the focus stays on item 40");
  }
```

(`step` binds `rank` = index + 1 and `name` = `P{index}`, as in Task 10's tests.)

In `check.rs` tests: a project whose `assets/ui/board.ui.ron` holds `BOARD` checks clean, and one with `List(count: 3)` reports `List needs "row_height"` at its position.

- [ ] **Step 2: Run them to see them fail.**
- [ ] **Step 3: Implement** by the rules.
- [ ] **Step 4: Run the tests** — `cargo test -p ryo2d --lib core::ui` passes.
- [ ] **Step 5: Run the gate and commit**:

```bash
git add src/core/ui/file/vocab.rs src/core/ui/file/build.rs src/core/ui/file/check.rs src/core/ui/nodes/list.rs src/core/ui/nodes/reconcile.rs
git commit -m "feat: List in UI files, with rows from a template

- List(count, row_height, row: Template) builds its pooled rows from the file
- A changed row shape rebuilds the rows in place and rebinds them, keeping the focus"
```

---

## Part 3: Acceptance and docs

### Task 12: List benches

**Files:**
- Modify: `benches/ui.rs`
- Test: the benches

**Interfaces:**
- Consumes: Tasks 7-10.

Rules:
- Add, registered in `criterion_group!`:
  - `virtual_list_frame`: "ui frame, a 10,000-row virtual list": a closure `ui.list(10_000)` of buttons (the same row as `list_frame`), scrolled to its middle (with `list_scroll_to` and settle frames, asserting the middle is in range before measuring). **Target: under 50 µs.**
  - `virtual_list_100_frame`: the same with 100 rows scrolled to its middle. **Target: the 10,000-row bench within 10% of it.**
  - `node_list_frame`: "ui frame, a 10,000-row node list": a node `List` of the same rows, bound each frame from `UiListBind` messages (`ui_set` per bound row, as a game would), scrolled to its middle. **Target: within `max(10%, 10 µs)` of `virtual_list_frame`.**
- Run them in a quiet run and record the numbers in the report. If a target is missed, profile (as in Task 6) and fix within Tasks 7-10's code; if still missed after one round, record the numbers and the reason (the controller rules on it).

- [ ] **Step 1: Add the benches.**
- [ ] **Step 2: Run them** (`cargo bench --bench ui -- "list"`) and compare with the targets.
- [ ] **Step 3: Run the gate and commit**:

```bash
git add benches/ui.rs
git commit -m "test: benches for virtual lists in both styles

- A 10,000-row closure list, the same with 100 rows, and a 10,000-row node list"
```

---

### Task 13: The Big list demo page

**Files:**
- Create: `examples/demos/ui/src/scenes/big_list_page.rs`, `examples/demos/ui/assets/ui/big_list.ui.ron`
- Modify: `examples/demos/ui/src/scenes/mod.rs`, `examples/demos/ui/src/lib.rs` (the page after the Mixed page), `examples/demos/ui/tests/acceptance.rs`
- Test: `examples/demos/ui/tests/acceptance.rs`

**Interfaces:**
- Consumes: Tasks 7-11; the `ui` demo's page pattern and 4d's file-page patterns (paths in consts, build before spawn, a load failure drawn as closure text and logged once).

Rules:
- A deterministic leaderboard of 10,000 entries (rank, name, score), generated in code (for example names from two small word lists by index, scores descending), shared by both lists.
- The left half: a closure `ui.list(10_000)` with a row of rank, name and score. The right half: the same leaderboard as a node `List` from `big_list.ui.ron` (a `LeaderRow` template with `{rank}`, `{name}`, `{score}` patterns), filled from `UiListBind` in the page's `ui`.
- Above them: a "jump to rank" text field and a Go button that call `list_focus` / `ctx.ui_list_focus` on both lists for the rank (clamped to 1..=10,000).
- Each list is in its own focus scope; Tab or the bumpers move between them, as the demo's other pages do.
- **Acceptance tests** (`examples/demos/ui/tests/acceptance.rs`), for each list:
  - from the top, the pad walks down 30 rows: the focus lands on rank 31 and the list has scrolled (rank 1's row is no longer in view);
  - a jump to rank 9,990 focuses it; the pad then walks down to rank 10,000 and further presses stay on it;
  - the rows drawn after the jump show the right ranks (read the node rows' labels, and the closure list's focused rank).

The page exposes what the tests read, as the inventory page does: `BigListPage::shown(&self) -> BigListShown` with, for each list, the focused rank (`Option<usize>`) and the first rank drawn in view. The tests wrap the page in a watcher that copies `shown()` into an `Arc<Mutex<..>>` each frame (the `WatchedInventory` pattern in `acceptance.rs`), and drive it with `pad` and `engine.tap_key`/typing for the jump field. The skeleton (fill in the jump-field typing with the engine's text input helper the text-input tests use):

```rust
#[derive(Clone, Default)]
struct ShownLists { closure: (Option<usize>, usize), node: (Option<usize>, usize) } // (focused rank, first rank in view)

struct WatchedBigList { page: BigListPage, shown: Arc<Mutex<ShownLists>> }
// impl DemoPage for WatchedBigList: delegate everything to `page`, and after `page.ui(..)` copy page.shown() into `shown`

fn big_list() -> Option<(HeadlessEngine, impl Fn() -> ShownLists)> {
  let shown = Arc::new(Mutex::new(ShownLists::default()));
  let watched = Arc::clone(&shown);
  let demo = Demo::new("Big list").resolution(1024, 768)
    .page(move || WatchedBigList { page: BigListPage::new(), shown: Arc::clone(&watched) });
  let mut engine = HeadlessEngine::new_or_skip(demo.engine_builder())?;
  engine.step_frames(6).unwrap();
  Some((engine, move || shown.lock().map(|s| s.clone()).unwrap_or_default()))
}

#[test]
fn both_big_lists_walk_by_pad_and_jump_to_a_rank() {
  let Some((mut engine, shown)) = big_list() else { return };
  for which in [0, 1] {
    // Focus the list (Tab or a bumper to the second list for which == 1), then walk 30 rows
    if which == 1 { pad(&mut engine, GamepadButton::RightTrigger); }
    for _ in 0..30 { pad(&mut engine, GamepadButton::DPadDown); }
    let (focused, first) = if which == 0 { shown().closure } else { shown().node };
    assert_eq!(focused, Some(31), "list {which}: rank 31 after 30 steps down");
    assert!(first > 1, "list {which}: scrolled past rank 1");
  }
  // Jump to rank 9,990 with the field and Go, then walk to the end
  jump_to(&mut engine, 9990);
  for which in [0, 1] {
    let focused = if which == 0 { shown().closure.0 } else { shown().node.0 };
    assert_eq!(focused, Some(9990), "list {which}: the jump focused rank 9,990");
  }
  for _ in 0..20 { pad(&mut engine, GamepadButton::DPadDown); }
  assert_eq!(shown().closure.0.or(shown().node.0), Some(10_000), "the walk stops at the last rank");
}
```

(Adjust the pad buttons to the page's scope switching; `jump_to` focuses the field, types the number and confirms Go.)

- [ ] **Step 1: Write the acceptance tests**, then the page and the file, and run `cargo test -p ui --test acceptance`.
- [ ] **Step 2: Capture the page** (`./dev.sh demos:tour` or a headless capture of the page) and look at it: two lists side by side, rows aligned, no blank rows after the first frame.
- [ ] **Step 3: Run the gate and commit**:

```bash
git add examples/demos/ui/src/scenes/big_list_page.rs examples/demos/ui/assets/ui/big_list.ui.ron examples/demos/ui/src/scenes/mod.rs examples/demos/ui/src/lib.rs examples/demos/ui/tests/acceptance.rs
git commit -m "feat: the ui demo's Big list page

- A 10,000-entry leaderboard as a closure list beside a file-built node list
- Jump to a rank; both lists are walkable by pad end to end"
```

---

### Task 14: Documentation

**Files:**
- Modify: `.claude/skills/ryo-ui/SKILL.md` and `src/cli/skills/ryo-ui.md`, `.claude/skills/ryo-api/SKILL.md` and `src/cli/skills/ryo-api.md`, `.claude/skills/ryo-messages/SKILL.md` and `src/cli/skills/ryo-messages.md`, `src/core/ui/AGENTS.md`, `src/core/text/AGENTS.md` (the text key), `docs/architecture/overview.md`
- Test: `cargo test -p ryo2d --lib embedded_skills_are_their_sources` and `cargo test -p ryo2d --doc`

Contents (read the code, not this plan, for the final names):
- **`ryo-ui`**: a "Virtual lists" section: when to use a list (more rows than fit; fixed row height), the closure `ui.list` (options, `ListResponse`, `list_scroll_to`, `list_focus`), the node `List` (code and file forms, the pool, `UiListBind` and where to handle it: in `ui`, or an observer, or `update`, and why a row draws from the frame after it is bound; per-row binding scope; the game API; `ui_list_index` for events), the focus rules (the focused row stays built; the pad walks the whole list; jumping), and what is lost when a row leaves the built range (its widget memory). Recipes: a leaderboard (closure and node) and an inventory grid (`columns`).
- **`ryo-api`**: the new signatures (`List`, `ListResponse`, `UiContext::{list, list_scroll_to, list_focus}`, `UiNode::{list, columns, overscan, row}`, `UiListRow`, `UiListBind`, the `SceneContext` and `UiHarness` list methods). **`ryo-messages`**: `UiListBind`.
- **`src/core/ui/AGENTS.md`**: the shared hasher (`core::hash`) and why SipHash is not used for engine keys; text keys (hashed once, confirmed on hit); the kept buffers; the layout diff by hash; lists (built rows, spacers, ids by index, the pool and its timing, the binding scope); the new guard and budgets.
- **`src/core/text/AGENTS.md`**: `TextKey` and the keyed engine calls.
- **`docs/architecture/overview.md`**: lists in the UI section.

- [ ] **Step 1: Write the docs**; after each skill, `cp` and `cmp`.
- [ ] **Step 2: Run the checks** — the embedded skills test and the doc tests pass.
- [ ] **Step 3: Run the gate and commit**:

```bash
git add .claude/skills/ryo-ui/SKILL.md src/cli/skills/ryo-ui.md .claude/skills/ryo-api/SKILL.md src/cli/skills/ryo-api.md .claude/skills/ryo-messages/SKILL.md src/cli/skills/ryo-messages.md src/core/ui/AGENTS.md src/core/text/AGENTS.md docs/architecture/overview.md
git commit -m "docs: virtual lists and the cheap UI frame

- The ui skill's virtual lists section and recipes; API and message references
- Module notes for the hasher, text keys, buffers, the layout diff, lists and the new guard"
```

---

### Task 15: Measure again and recommend

**Files:**
- Modify: `docs/backlog/ui-retained-nodes.md`
- Test: none (measurement)

Rules (the spec's "The end of 4e"):
- Profile the closure settings, node settings, plain 1,000-row list and the new list benches with xctrace Time Profiler (build with `CARGO_PROFILE_BENCH_DEBUG=true cargo bench --bench ui --no-run`; record each with `xctrace record --template 'Time Profiler' --launch -- <bench binary> --bench --profile-time 10 "<filter>"`; export the time-profile table and aggregate inclusive and self time by function, as the spec's profile did). Keep the scratch in `/private/tmp/claude-501/4e-profile-after/`.
- Write into the backlog note's 4e section:
  - the before and after bench numbers (from Tasks 1, 6 and 12);
  - the new profile's buckets for the node settings frame: how much of it is the node walk and the builders that subtree caching would skip, against what caching would have to track (component changes, interaction state, tweens, theme changes, layout above the subtree);
  - a recommendation, in a few sentences, for or against node subtree caching now, with the numbers that decide it. The owner decides.
- Move 4e to done in the note (what landed), and list what 4e left: measured rows, horizontal lists, subtree caching (pending the decision), the dropdown and choice lists virtualised, touch flinging.

- [ ] **Step 1: Profile** as above.
- [ ] **Step 2: Write the note.**
- [ ] **Step 3: Commit**:

```bash
git add docs/backlog/ui-retained-nodes.md
git commit -m "docs: 4e's numbers and the subtree caching recommendation

- Before and after frame numbers; the new profile of the node settings frame
- Whether node subtree caching is worth its machinery now"
```
