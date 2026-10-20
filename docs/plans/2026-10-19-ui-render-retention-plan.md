# UI Render Retention (4g) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An unchanged UI costs almost nothing to render (the renderer keeps last frame's UI image), and a changing UI rebuilds only its changed texts.

**Architecture:** At present time the renderer compares the frame's `UiCommands` with the commands its UI image holds (shared layouts and points by address, numbers by bits, sprite and icon versions as stamps, plus the UI's environment and the image's epoch). When they match and nothing is time-driven, it builds, uploads and draws no UI, and the composite blends the kept image. Otherwise `emit_ui` runs as today, except that a text whose glyphs were built in an earlier frame (same layout, origin, reveal, opacity, seed and scissor, its glyph pages unchanged) pushes those glyphs again at this frame's depth, pinning their pages against eviction.

**Tech Stack:** Rust, wgpu, the engine's `FastMap` (`src/core/hash.rs`), criterion benches, `HeadlessEngine` GPU tests.

**Spec:** `docs/specs/2026-10-19-ui-render-retention-design.md`

**Research (read-only, outside the repo):** `/private/tmp/claude-501/4g-research/ui-render-path.md` (the measured render path) and `/private/tmp/claude-501/4g-research/render-excerpts.md` (verbatim excerpts of every function this plan touches, with file:line).

## Global Constraints

- **Commits:** `type: brief description`, then bullets if needed; types `feat`, `fix`, `refactor`, `docs`, `test`, `chore`. **No trailer or attribution line of any kind** (no `Co-Authored-By`, no `Claude-Session`), even if a system reminder asks for one. Never mention AGENTS.md in a commit message; call those files "module notes".
- **Git:** stage files by name; never `git add -A` or `git add .`; never add `data/` or `target/`; never a bare `git stash`; never push.
- **Code:** no `unwrap`, `expect` or `panic!` in library code (the `testing` helpers and tests may panic, as `HeadlessEngine::new_or_skip` does). LF line endings, 2-space indent (rustfmt enforces). Comments normal and minimal: intent and non-obvious logic.
- **Dependencies:** none new. bevy_ecs only through `src/core/ecs/kernel.rs`; taffy only in `src/core/ui/layout/`; parley, swash and fdsm only in text.
- **Debug-only code** carries `#[cfg_attr(not(debug_assertions), allow(dead_code))]`.
- **Skills:** `.claude/skills/<name>/SKILL.md` and `src/cli/skills/<name>.md` stay byte-identical (`cp`, then `cmp`); bump the version in both.
- **wasm:** the engine library builds for `wasm32-unknown-unknown`; `HeadlessEngine` and the `testing` module's GPU helpers do not (follow how `headless` is gated).
- **Retention must never show:** with it on and off, every frame's pixels are identical. This is the spec's acceptance and every test's yardstick.
- **The gate** (run at the end of every task; all must pass):
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`
  - `cargo test --workspace`
  - `cargo check --release -p ryo2d --lib`
- **Disk:** a long plan's worktree `target/` reached 192 GB in 4f. After each task run `du -sh target`; above 100 GB, run `cargo clean` before the next task.

## Review Focus

1. **A frame that is not presented while the UI is idle** (surface lost, timeout, `FrameError::Skip`): the next presented frame must draw the UI, never a blank or older image. Pinned by `an_unpresented_frame_redraws` (Task 2).
2. **A game disabling, then re-enabling, the `ui` pass while its UI is unchanged:** no UI while disabled, the right UI after. Pinned by `a_disabled_ui_pass_hides_the_kept_ui` (Task 3).
3. **Swapping the `TextIcons` set (keyboard to gamepad prompts) under unchanged texts:** the new icons show the next frame. Pinned by `swapped_text_icons_show_at_once` (Task 5).
4. **World text churning the glyph atlas while the UI image is kept, then one UI change:** every reused text still samples its own glyphs. Pinned by `world_text_churn_under_a_kept_ui_matches` (Task 5).
5. **A UI alternating between two looks every other frame** (a blinking caret, a ticking label): every frame matches the render without retention. Pinned by `a_ui_alternating_every_frame_matches` (Task 5).

---

## File Structure

| File | Responsibility |
|---|---|
| `src/core/graphics/glyph_atlas.rs` (modify) | Fast maps; per-page generations (process-unique), page pins, eviction and refusal counters |
| `src/core/graphics/atlas.rs`, `src/core/graphics/sprite_gpu.rs` (modify) | Pass the glyph page generation, pin and counters through to the renderer |
| `src/core/render/ui_retain/mod.rs` (create) | `UiRetain` (keep the image or not), `UiRenderEnv`, `UiRenderStats`, command equality and stamps |
| `src/core/render/ui_retain/texts.rs` (create) | `UiTextCache`: texts' built glyphs by key, validated by page generation, pinned on reuse |
| `src/core/render/ui_retain/tests.rs` (create) | GPU-free unit tests of the decision and keys |
| `src/core/render/ui_retain/gpu_tests.rs` (create) | In-crate GPU tests needing crate access (a sprite updated in place) |
| `src/core/render/ui.rs` (modify) | `emit_ui` takes the environment and the text cache |
| `src/core/render/text.rs` (modify) | `emit_text` split into `emit_glyphs` and `emit_icons` |
| `src/core/render/draw_list.rs`, `src/core/render/frame.rs` (modify) | `push_glyphs`, `glyph_count`, `glyphs_from`; `PixelRect: Hash`; `FrameScene::ui_kept` |
| `src/core/graphics/compositor.rs`, `src/core/graphics/renderer.rs` (modify) | The UI image's epoch; a kept frame composites the image without the `ui` pass |
| `src/core/engine/extract.rs`, `src/core/engine/mod.rs`, `src/core/config/debug.rs` (modify) | Wiring, the `ui_retain` switch, the stats |
| `src/core/testing/retain.rs` (create), `src/core/testing/headless.rs`, `src/core/testing/mod.rs` (modify) | `PixelPair` (the on/off pixel oracle), `ui_render_stats`, `render_time` |
| `tests/e2e/ui_retain.rs` (create), `tests/e2e.rs` (modify) | Engine-level retention tests |
| `examples/demos/ui/tests/retain.rs`, `examples/games/breakout/tests/retain.rs` (create) | The demo and game under the oracle |
| `src/core/console/builtins.rs`, `src/core/ui/debug.rs`, `src/core/engine/frame.rs` (modify) | `ui:retain`, the overlay's render line |
| `benches/ui_render.rs` (create), `benches/text.rs`, `Cargo.toml` (modify) | GPU benches and the guard |
| Docs (modify) | Skills `ryo-ui`, `ryo-api`, `ryo-console`; `src/core/render/AGENTS.md`; the spec; `docs/backlog/ui-retained-nodes.md` |

---

### Task 1: The glyph atlas on the fast hasher, with page generations and pins

**Files:**
- Modify: `src/core/graphics/glyph_atlas.rs` (maps at `:91`, `ShelfPage` at `:79-82`, `add_page` at `:117-120`, `place` at `:124-181`, `GlyphAtlas` at `:224-238`; tests at `:400+`)
- Modify: `src/core/graphics/atlas.rs` (`TextureAtlas`, `glyphs: GlyphAtlas` at `:364`)
- Modify: `src/core/graphics/sprite_gpu.rs` (`SpriteGpuRenderer`, `glyph` at `:593-602`, `frame` field)

**Interfaces:**
- Produces (used by Tasks 3 and 4):
  - `GlyphShelves::page_generation(&self, format: GlyphFormat, index: usize) -> Option<u64>`
  - `GlyphShelves::pin(&mut self, format: GlyphFormat, index: usize, frame: u64)`
  - `GlyphShelves::evictions(&self) -> u64`, `GlyphShelves::refused(&self) -> u64`
  - `GlyphAtlas::page_generation(&self, page: GlyphPage) -> Option<u64>`, `GlyphAtlas::pin(&mut self, page: GlyphPage, frame: u64)`, `GlyphAtlas::evictions(&self) -> u64`, `GlyphAtlas::refused(&self) -> u64`
  - `TextureAtlas::glyph_page_generation(&self, page: GlyphPage) -> Option<u64>`, `TextureAtlas::pin_glyph_page(&mut self, page: GlyphPage, frame: u64)`, `TextureAtlas::glyph_evictions(&self) -> u64`, `TextureAtlas::glyphs_refused(&self) -> u64`
  - `SpriteGpuRenderer::glyph_page_generation(&self, page: GlyphPage) -> Option<u64>`, `SpriteGpuRenderer::pin_glyph_page(&mut self, page: GlyphPage)` (stamps with its current `frame`), `SpriteGpuRenderer::glyph_evictions(&self) -> u64`, `SpriteGpuRenderer::glyphs_refused(&self) -> u64`

- [ ] **Step 1: Capture the demo tour baseline twice, before any change**

The final task compares the demo tour against this. Two captures show which pages are nondeterministic.

```bash
./dev.sh demos:tour
rm -rf /private/tmp/claude-501/4g-tour && mkdir -p /private/tmp/claude-501/4g-tour
cp -R target/demo-tour /private/tmp/claude-501/4g-tour/base-1
./dev.sh demos:tour
cp -R target/demo-tour /private/tmp/claude-501/4g-tour/base-2
diff -rq /private/tmp/claude-501/4g-tour/base-1 /private/tmp/claude-501/4g-tour/base-2 > /private/tmp/claude-501/4g-tour/nondeterministic.txt || true
wc -l /private/tmp/claude-501/4g-tour/nondeterministic.txt
```

Expected: both captures complete; `nondeterministic.txt` lists the pages that differ between two runs (possibly none).

- [ ] **Step 2: Write the failing tests**

Add to the `tests` module of `src/core/graphics/glyph_atlas.rs`:

```rust
  /// Two pages of four 12x12 glyphs each: keys 0..4 drawn in frame 1 fill
  /// page 0, keys 4..8 drawn in frame 2 fill page 1
  fn two_full_pages() -> GlyphShelves {
    let mut shelves = GlyphShelves::new(32, 2);
    shelves.add_page(GlyphFormat::Mask);
    shelves.add_page(GlyphFormat::Mask);
    for glyph in 0..8 {
      let frame = if glyph < 4 { 1 } else { 2 };
      let placed = shelves.place(&key(glyph), GlyphFormat::Mask, (12, 12), frame);
      assert!(matches!(placed, Placement::Placed { .. }), "{placed:?}");
    }
    shelves
  }

  #[test]
  fn a_pinned_page_is_not_evicted_in_its_frame() {
    let mut shelves = two_full_pages();
    // Page 0 is the least recently used, but pinned in frame 3
    shelves.pin(GlyphFormat::Mask, 0, 3);
    let placed = shelves.place(&key(100), GlyphFormat::Mask, (12, 12), 3);
    let Placement::Placed { page, .. } = placed else {
      panic!("{placed:?}")
    };
    assert_eq!(page, 1, "the unpinned page is cleared instead");
    assert!(shelves.lookup(&key(0), 3).is_some(), "the pinned page's glyphs stay");
    assert!(shelves.lookup(&key(4), 3).is_none());
  }

  #[test]
  fn an_eviction_changes_the_page_generation_and_is_counted() {
    let mut shelves = two_full_pages();
    let before = (
      shelves.page_generation(GlyphFormat::Mask, 0),
      shelves.page_generation(GlyphFormat::Mask, 1),
    );
    assert_eq!(shelves.evictions(), 0);
    // Page 0 is the least recently used: cleared for the new glyph
    shelves.place(&key(100), GlyphFormat::Mask, (12, 12), 3);
    assert_eq!(shelves.evictions(), 1);
    assert_ne!(shelves.page_generation(GlyphFormat::Mask, 0), before.0);
    assert_eq!(shelves.page_generation(GlyphFormat::Mask, 1), before.1);
    assert_eq!(shelves.page_generation(GlyphFormat::Mask, 2), None);
  }

  #[test]
  fn page_generations_never_repeat_across_atlases() {
    let mut first = GlyphShelves::new(32, 1);
    let mut second = GlyphShelves::new(32, 1);
    first.add_page(GlyphFormat::Mask);
    second.add_page(GlyphFormat::Mask);
    assert_ne!(
      first.page_generation(GlyphFormat::Mask, 0),
      second.page_generation(GlyphFormat::Mask, 0)
    );
  }

  #[test]
  fn a_glyph_refused_for_a_full_atlas_is_counted() {
    let mut shelves = two_full_pages();
    shelves.pin(GlyphFormat::Mask, 0, 2);
    // Frame 2: page 1 was drawn in it and page 0 is pinned in it
    assert!(matches!(
      shelves.place(&key(100), GlyphFormat::Mask, (12, 12), 2),
      Placement::Full
    ));
    assert_eq!(shelves.refused(), 1);
  }
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib graphics::glyph_atlas`
Expected: compile errors: no method `pin`, `page_generation`, `evictions`, `refused`.

- [ ] **Step 4: Implement**

In `src/core/graphics/glyph_atlas.rs`:

```rust
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::core::hash::{FastMap, FastSet};
```

```rust
struct ShelfPage {
  allocator: BucketedAtlasAllocator,
  keys: FastSet<GlyphKey>,
  /// Changes whenever the page is cleared, never repeating in the process
  /// (a new atlas's pages never match an old one's): what was built from
  /// its glyphs is good while it is unchanged
  generation: u64,
  /// The last frame something built from the page earlier was drawn
  /// without looking its glyphs up; the page is not evicted in that frame
  pinned: u64,
}

/// A page generation no page has had in this process
fn next_page_generation() -> u64 {
  static NEXT: AtomicU64 = AtomicU64::new(1);
  NEXT.fetch_add(1, Ordering::Relaxed)
}
```

`GlyphShelves`: `pages: FastMap<GlyphFormat, Vec<ShelfPage>>`, `entries: FastMap<GlyphKey, ShelfEntry>` (the hot map: looked up for every glyph every frame), and two new fields:

```rust
  /// Pages cleared to make room, over the atlas's life
  evictions: u64,
  /// Glyphs refused because every page held glyphs drawn this frame
  refused: u64,
```

`warned` and `warned_full` stay `HashSet` (cold). `new` initialises the maps with `FastMap::default()` and the counters with 0. `add_page` pushes `ShelfPage { allocator, keys: FastSet::default(), generation: next_page_generation(), pinned: 0 }`.

In `place`, the victim test counts the pin:

```rust
        let newest = page
          .keys
          .iter()
          .filter_map(|k| self.entries.get(k))
          .map(|e| e.last_used)
          .max()
          .unwrap_or(0)
          .max(page.pinned);
```

Before `return Placement::Full;` add `self.refused += 1;`. Where the victim page is cleared, add `page.generation = next_page_generation();` and, after the `if let`, `self.evictions += 1;`.

New methods on `GlyphShelves`:

```rust
  /// The generation of page `index` of `format` (`None`: no such page)
  pub(crate) fn page_generation(&self, format: GlyphFormat, index: usize) -> Option<u64> {
    Some(self.pages.get(&format)?.get(index)?.generation)
  }

  /// Keep page `index` of `format` from eviction in `frame`: something
  /// built from its glyphs earlier is drawn in it
  pub(crate) fn pin(&mut self, format: GlyphFormat, index: usize, frame: u64) {
    if let Some(page) = self.pages.get_mut(&format).and_then(|pages| pages.get_mut(index)) {
      page.pinned = page.pinned.max(frame);
    }
  }

  /// Pages cleared to make room so far
  pub(crate) fn evictions(&self) -> u64 {
    self.evictions
  }

  /// Glyphs refused so far because the pages were full of glyphs drawn in
  /// their frame
  pub(crate) fn refused(&self) -> u64 {
    self.refused
  }
```

`GlyphAtlas`: `pages: FastMap<GlyphFormat, Vec<GpuPage>>`, `empty: FastSet<GlyphKey>` (hit for every space glyph); pass-throughs:

```rust
  /// The generation of the glyph page `page` (`None`: no such page)
  pub(crate) fn page_generation(&self, page: GlyphPage) -> Option<u64> {
    self.shelves.page_generation(page.format, usize::from(page.index))
  }

  /// Keep `page` from eviction in `frame`
  pub(crate) fn pin(&mut self, page: GlyphPage, frame: u64) {
    self.shelves.pin(page.format, usize::from(page.index), frame);
  }

  /// Pages cleared to make room so far
  pub(crate) fn evictions(&self) -> u64 {
    self.shelves.evictions()
  }

  /// Glyphs refused so far for a full atlas
  pub(crate) fn refused(&self) -> u64 {
    self.shelves.refused()
  }
```

`TextureAtlas` (`atlas.rs`) passes each through to `self.glyphs` as `glyph_page_generation`, `pin_glyph_page(page, frame)`, `glyph_evictions`, `glyphs_refused`. `SpriteGpuRenderer` (`sprite_gpu.rs`) passes each through to `self.atlas`; its `pin_glyph_page(&mut self, page: GlyphPage)` passes `self.frame`, the frame `glyph` stamps with:

```rust
  /// Keep the glyph page `page` from eviction this frame: glyphs built from
  /// it in an earlier frame are drawn again
  pub(crate) fn pin_glyph_page(&mut self, page: GlyphPage) {
    self.atlas.pin_glyph_page(page, self.frame);
  }
```

Until Tasks 3 and 4 use them, the new methods are called only by tests (rustc counts a method called only from dead code as dead too): put `#[cfg_attr(not(test), allow(dead_code))] // used by UI render retention (Tasks 3 and 4)` on every new method in all three files, and remove each allow once its method is used (Tasks 3 and 4).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ryo2d --lib graphics::glyph_atlas`
Expected: PASS, the new tests and the existing ones.

- [ ] **Step 6: Measure the HUD bench and record it**

Run: `cargo bench --bench text -- hud_frame 2>&1 | tail -20`
Expected: the HUD frame below the research's 248.3 µs, by roughly the glyph lookup's 41 µs (load moves numbers 10 to 15%). Put the number in your report.

- [ ] **Step 7: Run the gate and commit**

Run the gate (Global Constraints). Then:

```bash
git add src/core/graphics/glyph_atlas.rs src/core/graphics/atlas.rs src/core/graphics/sprite_gpu.rs
git commit -m "refactor: the glyph atlas on the fast hasher, with page generations and pins" -m "- glyph lookups hash with the engine's fast hasher instead of SipHash
- each glyph page has a process-unique generation, changed when it is cleared
- a page can be pinned for a frame; evictions and refusals are counted"
```

---

### Task 2: The retention decision

**Files:**
- Create: `src/core/render/ui_retain/mod.rs`, `src/core/render/ui_retain/tests.rs`
- Modify: `src/core/render/mod.rs` (declare the module, export `UiRenderStats`)
- Modify: `src/core/ui/immediate/commands.rs` (`UiCommands::as_slice`)
- Modify: `src/core/text/layout.rs` (`TextLayout::has_effects`)

**Interfaces:**
- Consumes: `UiCommand`, `UiCommands` (`src/core/ui/immediate/commands.rs:13-134`), `UiSpace` (`src/core/render/ui.rs:46-57`, `Copy + PartialEq`), `TextIcons::get(&self, name: &str) -> Option<&IconSource>`, `IconSource { sprite, region: Option<Rect> }`, `Icon::{Sprite { sprite, .. }, Named { name, .. }}`, `PlacedIcon { icon, .. }`, `SpriteHandle::serial() -> usize`, `SpriteHandle::generation() -> u64`, `FastHasher` (`src/core/hash.rs`).
- Produces (used by Tasks 3, 4, 7):
  - `pub struct UiRenderStats { pub kept: bool, pub texts_built: u32, pub texts_reused: u32, pub glyph_evictions: u64 }` (`Debug, Clone, Copy, Default, PartialEq, Eq`), exported from `crate::core::render`
  - `pub(crate) struct UiRenderEnv { pub(crate) space: UiSpace, pub(crate) on_target: bool, pub(crate) text: bool, pub(crate) fonts: u64 }` (`Debug, Clone, Copy, PartialEq`)
  - `pub(crate) struct UiRetain` (`Debug, Default`) with `pub(crate) stats: UiRenderStats` and:
    - `keep(&mut self, commands: &UiCommands, icons: Option<&TextIcons>, env: UiRenderEnv, image: u64) -> bool`
    - `presented(&mut self, kept: bool, drew: bool, image: u64)`
    - `lost(&mut self)`
    - `clear(&mut self)`
  - `UiCommands::as_slice(&self) -> &[(u32, UiCommand)]`
  - `TextLayout::has_effects(&self) -> bool`

- [ ] **Step 1: Write the failing tests**

Create `src/core/render/ui_retain/tests.rs`:

```rust
use std::sync::Arc;

use glam::Vec2;

use super::*;
use crate::core::asset::SpriteHandle;
use crate::core::graphics::Sprite;
use crate::core::text::{GlyphStyle, Icon, PlacedIcon, TextIcons, TextLayout};
use crate::core::ui::Rect;
use crate::core::ui::immediate::commands::{UiCommand, UiCommands};
use crate::core::render::UiSpace;
use crate::core::types::Color;

fn env() -> UiRenderEnv {
  UiRenderEnv {
    space: UiSpace {
      size: Vec2::new(320.0, 240.0),
      px_scale: 1.0,
      image: (320, 240),
      mouse_scale: 1.0,
    },
    on_target: false,
    text: true,
    fonts: 1,
  }
}

fn rect(x: f32) -> UiCommand {
  UiCommand::Rect {
    rect: Rect::new(x, 10.0, 50.0, 20.0),
    fill: Color::WHITE,
    radius: 4.0,
    border: None,
    clip: None,
    layer: 0,
  }
}

fn text(layout: &Arc<TextLayout>) -> UiCommand {
  UiCommand::Text {
    layout: layout.clone(),
    origin: Vec2::new(8.0, 8.0),
    reveal: None,
    opacity: 1.0,
    seed: 3,
    clip: None,
    layer: 0,
  }
}

fn commands(list: &[UiCommand]) -> UiCommands {
  let mut commands = UiCommands::default();
  for (order, command) in list.iter().enumerate() {
    commands.push(order as u32, command.clone());
  }
  commands
}

/// A retainer whose image (epoch 7) holds `list`, drawn and presented
fn drawn(list: &[UiCommand]) -> UiRetain {
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&commands(list), None, env(), 7), "nothing drawn yet");
  retain.presented(false, true, 7);
  retain
}

#[test]
fn the_same_commands_keep_the_image() {
  let layout = Arc::new(TextLayout::default());
  let list = [rect(0.0), text(&layout)];
  let mut retain = drawn(&list);
  assert!(retain.keep(&commands(&list), None, env(), 7));
  assert!(retain.keep(&commands(&list), None, env(), 7), "and again");
}

#[test]
fn a_changed_number_redraws() {
  let mut retain = drawn(&[rect(0.0)]);
  assert!(!retain.keep(&commands(&[rect(0.5)]), None, env(), 7));
}

#[test]
fn another_order_redraws() {
  let mut retain = drawn(&[rect(0.0)]);
  let mut moved = UiCommands::default();
  moved.push(1, rect(0.0));
  assert!(!retain.keep(&moved, None, env(), 7));
}

#[test]
fn an_equal_layout_elsewhere_is_another_text() {
  let mut retain = drawn(&[text(&Arc::new(TextLayout::default()))]);
  let other = Arc::new(TextLayout::default());
  assert!(!retain.keep(&commands(&[text(&other)]), None, env(), 7));
}

#[test]
fn a_text_effect_never_keeps() {
  let layout = Arc::new(TextLayout {
    styles: vec![GlyphStyle {
      color: Color::WHITE,
      outline: None,
      shadow: None,
      glow: None,
      effect: Some("wave".to_string()),
    }],
    ..TextLayout::default()
  });
  assert!(layout.has_effects());
  let list = [text(&layout)];
  let mut retain = drawn(&list);
  for _ in 0..3 {
    assert!(!retain.keep(&commands(&list), None, env(), 7));
    retain.presented(false, true, 7);
  }
}

#[test]
fn another_environment_or_image_redraws() {
  let list = [rect(0.0)];
  let mut retain = drawn(&list);
  let scaled = UiRenderEnv {
    fonts: 2,
    ..env()
  };
  assert!(!retain.keep(&commands(&list), None, scaled, 7), "the fonts changed");
  retain.presented(false, true, 7);
  assert!(retain.keep(&commands(&list), None, scaled, 7));
  assert!(!retain.keep(&commands(&list), None, scaled, 8), "a new UI image");
}

#[test]
fn an_unpresented_frame_redraws() {
  let list = [rect(0.0)];
  let mut retain = drawn(&list);
  assert!(retain.keep(&commands(&list), None, env(), 7));
  retain.lost();
  assert!(!retain.keep(&commands(&list), None, env(), 7));
}

#[test]
fn a_frame_whose_ui_pass_did_not_draw_redraws() {
  let list = [rect(0.0)];
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&commands(&list), None, env(), 7));
  retain.presented(false, false, 7);
  assert!(!retain.keep(&commands(&list), None, env(), 7));
}

#[test]
fn a_kept_frame_leaves_the_image_as_drawn() {
  let list = [rect(0.0)];
  let mut retain = drawn(&list);
  assert!(retain.keep(&commands(&list), None, env(), 7));
  // The `ui` pass was disabled: the image is untouched, still the commands'
  retain.presented(true, false, 7);
  assert!(retain.keep(&commands(&list), None, env(), 7));
}

#[test]
fn no_commands_never_keep() {
  let mut retain = drawn(&[]);
  assert!(!retain.keep(&UiCommands::default(), None, env(), 7));
}

#[test]
fn a_sprite_updated_in_place_redraws() {
  let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 0, 0, 255), "swatch");
  let list = [UiCommand::Sprite {
    sprite: sprite.clone(),
    rect: Rect::new(0.0, 0.0, 4.0, 4.0),
    uv: [0.0, 0.0, 1.0, 1.0],
    tint: Color::WHITE,
    filter: Default::default(),
    clip: None,
    layer: 0,
  }];
  let mut retain = drawn(&list);
  assert!(retain.keep(&commands(&list), None, env(), 7));
  sprite.update(Sprite::solid_rect(4, 4, 0, 0, 255, 255));
  assert!(!retain.keep(&commands(&list), None, env(), 7));
}

#[test]
fn a_renamed_icon_redraws() {
  let red = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 0, 0, 255), "red");
  let blue = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 0, 0, 255, 255), "blue");
  let mut icons = TextIcons::default();
  icons.insert("confirm", red);
  let layout = Arc::new(TextLayout {
    icons: vec![PlacedIcon {
      icon: Icon::Named {
        name: "confirm".to_string(),
        size: None,
      },
      rect: Rect::new(0.0, 0.0, 4.0, 4.0),
      cluster: 0,
    }],
    ..TextLayout::default()
  });
  let list = [text(&layout)];
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&commands(&list), Some(&icons), env(), 7));
  retain.presented(false, true, 7);
  assert!(retain.keep(&commands(&list), Some(&icons), env(), 7));
  icons.insert("confirm", blue);
  assert!(!retain.keep(&commands(&list), Some(&icons), env(), 7));
}
```

Adjust the `use` paths to where each type actually lives (the compiler names them); the test bodies stay as written. `Filter`'s default is `Filter::Default`; if `Filter` has no `Default` impl, write `Filter::Default`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib render::ui_retain`
Expected: compile errors: no module `ui_retain` / no `UiRetain`.

- [ ] **Step 3: Implement**

`src/core/ui/immediate/commands.rs`, in `impl UiCommands`:

```rust
  /// Each command with its order, in the order they were pushed
  pub(crate) fn as_slice(&self) -> &[(u32, UiCommand)] {
    &self.commands
  }
```

`src/core/text/layout.rs`, in `impl TextLayout` (add the `impl` block if there is none):

```rust
  /// Whether drawing it runs a text effect (wave, shake, rainbow or a game's
  /// own): effects may read the clock, so it looks different every frame
  pub(crate) fn has_effects(&self) -> bool {
    self.styles.iter().any(|style| style.effect.is_some())
  }
```

`src/core/render/mod.rs`: declare `pub(crate) mod ui_retain;` beside `pub(crate) mod ui;`, and export `pub use ui_retain::UiRenderStats;`.

Create `src/core/render/ui_retain/mod.rs`:

```rust
//! UI render retention: a frame whose UI draws exactly what the UI image
//! holds keeps the image, building, uploading and drawing no UI (see the
//! module notes, "UI render retention")

use std::hash::Hasher;
use std::sync::Arc;

use crate::core::hash::FastHasher;
use crate::core::render::UiSpace;
use crate::core::text::{Icon, TextIcons};
use crate::core::ui::Rect;
use crate::core::ui::immediate::commands::{UiCommand, UiCommands};

#[cfg(test)]
mod tests;

/// What the UI's render step did in a frame
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiRenderStats {
  /// The UI image was kept from the last frame: no UI was built, uploaded
  /// or drawn
  pub kept: bool,
  /// Texts whose glyphs were built
  pub texts_built: u32,
  /// Texts drawn from glyphs built in an earlier frame
  pub texts_reused: u32,
  /// Glyph pages cleared to make room since the renderer started
  pub glyph_evictions: u64,
}

/// What a frame's UI pixels depend on besides its commands
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UiRenderEnv {
  pub(crate) space: UiSpace,
  /// Rects snap to whole pixels of the pixel-art target
  pub(crate) on_target: bool,
  /// Text is drawn (the game kept the text resources)
  pub(crate) text: bool,
  /// The text engine's font generation (faces and render modes)
  pub(crate) fonts: u64,
}

/// Whether the UI image can be shown again as it is, and what it holds
#[derive(Debug, Default)]
pub(crate) struct UiRetain {
  /// The commands the UI image holds or is being drawn with: copies, so the
  /// layouts and points they share stay alive and no address in them is
  /// reused while compared
  drawn: Vec<(u32, UiCommand)>,
  /// Each drawn command's stamp (`stamp_of`)
  stamps: Vec<u64>,
  env: Option<UiRenderEnv>,
  /// The epoch of the UI image `drawn` is in, once a presented frame's `ui`
  /// pass drew it
  image: Option<u64>,
  /// The last frame's
  pub(crate) stats: UiRenderStats,
}

impl UiRetain {
  /// Whether this frame shows the UI image as it is: its commands draw what
  /// the image holds (the same commands in the same order, the same sprite
  /// and icon versions, none time-driven), in the same environment, and
  /// the image (epoch `image`) is the one they were drawn into. When not,
  /// the commands become what the image is to hold, once drawn.
  pub(crate) fn keep(
    &mut self,
    commands: &UiCommands,
    icons: Option<&TextIcons>,
    env: UiRenderEnv,
    image: u64,
  ) -> bool {
    let now = commands.as_slice();
    let same = !now.is_empty()
      && self.image == Some(image)
      && self.env == Some(env)
      && now.len() == self.drawn.len()
      && now
        .iter()
        .zip(&self.drawn)
        .zip(&self.stamps)
        .all(|(((order, command), (was_order, was)), stamp)| {
          order == was_order
            && !animated(command)
            && same(command, was)
            && stamp_of(command, icons) == *stamp
        });
    if same {
      return true;
    }
    self.drawn.clear();
    self.drawn.extend_from_slice(now);
    self.stamps.clear();
    self
      .stamps
      .extend(now.iter().map(|(_, command)| stamp_of(command, icons)));
    self.env = Some(env);
    self.image = None;
    false
  }

  /// The frame was presented. Unless it kept the image, the image now holds
  /// its commands if its `ui` pass drew them (`drew`) into the image of
  /// epoch `image`, and is unknown otherwise.
  pub(crate) fn presented(&mut self, kept: bool, drew: bool, image: u64) {
    if !kept {
      self.image = drew.then_some(image);
    }
  }

  /// The frame was not presented: what the image holds is unknown
  pub(crate) fn lost(&mut self) {
    self.image = None;
  }

  /// Forget the image (retention was turned off)
  pub(crate) fn clear(&mut self) {
    self.drawn.clear();
    self.stamps.clear();
    self.env = None;
    self.image = None;
  }
}

/// Whether drawing `command` reads the clock: a text with an effect
fn animated(command: &UiCommand) -> bool {
  matches!(command, UiCommand::Text { layout, .. } if layout.has_effects())
}

fn bits(rect: Rect) -> [u32; 4] {
  [rect.x, rect.y, rect.width, rect.height].map(f32::to_bits)
}

/// Whether `a` draws what `b` does: shared layouts and points by address,
/// numbers by their bits, sprites by handle (their versions are stamps)
fn same(a: &UiCommand, b: &UiCommand) -> bool {
  let clips = |a: &Option<Rect>, b: &Option<Rect>| a.map(bits) == b.map(bits);
  match (a, b) {
    (
      UiCommand::Rect {
        rect,
        fill,
        radius,
        border,
        clip,
        layer,
      },
      UiCommand::Rect {
        rect: rect_b,
        fill: fill_b,
        radius: radius_b,
        border: border_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      bits(*rect) == bits(*rect_b)
        && fill == fill_b
        && radius.to_bits() == radius_b.to_bits()
        && border.map(|(width, color)| (width.to_bits(), color))
          == border_b.map(|(width, color)| (width.to_bits(), color))
        && clips(clip, clip_b)
        && layer == layer_b
    }
    (
      UiCommand::Sprite {
        sprite,
        rect,
        uv,
        tint,
        filter,
        clip,
        layer,
      },
      UiCommand::Sprite {
        sprite: sprite_b,
        rect: rect_b,
        uv: uv_b,
        tint: tint_b,
        filter: filter_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      sprite.serial() == sprite_b.serial()
        && bits(*rect) == bits(*rect_b)
        && uv.map(f32::to_bits) == uv_b.map(f32::to_bits)
        && tint == tint_b
        && filter == filter_b
        && clips(clip, clip_b)
        && layer == layer_b
    }
    (
      UiCommand::Line {
        points,
        at,
        width,
        color,
        closed,
        clip,
        layer,
      },
      UiCommand::Line {
        points: points_b,
        at: at_b,
        width: width_b,
        color: color_b,
        closed: closed_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      Arc::ptr_eq(points, points_b)
        && at.to_array().map(f32::to_bits) == at_b.to_array().map(f32::to_bits)
        && width.to_bits() == width_b.to_bits()
        && color == color_b
        && closed == closed_b
        && clips(clip, clip_b)
        && layer == layer_b
    }
    (
      UiCommand::Text {
        layout,
        origin,
        reveal,
        opacity,
        seed,
        clip,
        layer,
      },
      UiCommand::Text {
        layout: layout_b,
        origin: origin_b,
        reveal: reveal_b,
        opacity: opacity_b,
        seed: seed_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      Arc::ptr_eq(layout, layout_b)
        && origin.to_array().map(f32::to_bits) == origin_b.to_array().map(f32::to_bits)
        && reveal.map(|(shown, fade)| (shown.to_bits(), fade.to_bits()))
          == reveal_b.map(|(shown, fade)| (shown.to_bits(), fade.to_bits()))
        && opacity.to_bits() == opacity_b.to_bits()
        && seed == seed_b
        && clips(clip, clip_b)
        && layer == layer_b
    }
    _ => false,
  }
}

/// What `command` draws from besides its own value: a sprite's version, or
/// a text's icons (each sprite's version; a name's sprite and region in
/// `icons`); 0 for neither
fn stamp_of(command: &UiCommand, icons: Option<&TextIcons>) -> u64 {
  match command {
    UiCommand::Sprite { sprite, .. } => sprite.generation(),
    UiCommand::Text { layout, .. } if !layout.icons.is_empty() => {
      let mut hasher = FastHasher::default();
      for placed in &layout.icons {
        match &placed.icon {
          Icon::Sprite { sprite, .. } => {
            hasher.write_usize(sprite.serial());
            hasher.write_u64(sprite.generation());
          }
          Icon::Named { name, .. } => match icons.and_then(|icons| icons.get(name)) {
            Some(source) => {
              hasher.write_usize(source.sprite.serial());
              hasher.write_u64(source.sprite.generation());
              for bit in source.region.map_or([u32::MAX; 4], bits) {
                hasher.write_u32(bit);
              }
            }
            None => hasher.write_u64(u64::MAX),
          },
        }
      }
      hasher.finish()
    }
    _ => 0,
  }
}
```

Until Task 3 wires it in, the module is used only by its tests: put `#![cfg_attr(not(test), allow(dead_code))] // wired into the frame in the next task` at the top of `ui_retain/mod.rs`, and the same (as an outer `#[cfg_attr(..)]`) on `UiCommands::as_slice` and `TextLayout::has_effects`, which only dead code calls yet. Task 3 removes all three.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib render::ui_retain`
Expected: PASS (12 tests).

- [ ] **Step 5: Run the gate and commit**

```bash
git add src/core/render/ui_retain/mod.rs src/core/render/ui_retain/tests.rs src/core/render/mod.rs src/core/ui/immediate/commands.rs src/core/text/layout.rs
git commit -m "feat: the UI render retention decision" -m "- a frame keeps the UI image when its commands, their sprite and icon versions, the UI's environment and the image are those it was drawn with
- text effects and unpresented frames always redraw"
```

---

### Task 3: An unchanged UI keeps its image

**Files:**
- Modify: `src/core/graphics/compositor.rs` (`create_ui_texture` callers, `resize_ui` at `:241-245`)
- Modify: `src/core/graphics/renderer.rs` (`render_frame` at `:721-805`, `render_blank_frame`, the struct's fields)
- Modify: `src/core/render/frame.rs` (`FrameScene` at `:74-96`)
- Modify: `src/core/engine/extract.rs` (the UI section `:331-380`, the frame result `:427-466`)
- Modify: `src/core/engine/mod.rs` (engine fields `:210-218`, `Engine::new` `:658`, `EngineBuilder::ui_cache` `:1217-1224`)
- Modify: `src/core/config/debug.rs` (`DebugConfig` `:230-285`, the doc table `:8-20`)
- Create: `src/core/testing/retain.rs`
- Modify: `src/core/testing/headless.rs`, `src/core/testing/mod.rs`
- Create: `tests/e2e/ui_retain.rs`; Modify: `tests/e2e.rs`

**Interfaces:**
- Consumes: Task 1's `SpriteGpuRenderer::glyph_evictions()`; Task 2's `UiRetain`, `UiRenderEnv`, `UiRenderStats`.
- Produces:
  - `Compositor::ui_epoch(&self) -> u64`; `WgpuRenderer::ui_image_epoch(&self) -> u64`; `WgpuRenderer::ui_drawn(&self) -> bool`
  - `FrameScene::ui_kept: bool`
  - `DebugConfig::ui_retain: bool` (default true); `EngineBuilder::ui_retain(self, on: bool) -> Self`
  - Engine field `ui_retain: UiRetain`; `Engine::ui_render_stats(&self) -> UiRenderStats`; `Engine::render_time(&self) -> Duration`
  - `HeadlessEngine::ui_render_stats(&self) -> UiRenderStats` and `HeadlessEngine::render_time(&self) -> Duration`, both `#[doc(hidden)]`
  - `ryo2d::testing::{PixelPair, UiRenderStats}`; `PixelPair::new(make: impl Fn() -> EngineBuilder) -> Option<PixelPair>`, `pub on: HeadlessEngine`, `pub off: HeadlessEngine`, `each(&mut self, f: impl FnMut(&mut HeadlessEngine))`, `frames(&mut self, n: u32, what: &str)`, `compared(&self) -> usize`, `kept(&self) -> usize`, `reused(&self) -> usize`

- [ ] **Step 1: Write the pixel oracle helper**

It is test support, used by this task's tests and every later one. Create `src/core/testing/retain.rs`:

```rust
//! Two engines alike but for UI render retention, compared pixel for pixel

use crate::core::engine::EngineBuilder;

use super::{HeadlessEngine, StepOutcome};

/// Two engines built alike but for UI render retention (on, off), driven
/// together and compared after every frame: retention must never show.
/// For the retention tests; panics on any difference.
#[doc(hidden)]
pub struct PixelPair {
  /// Retention on
  pub on: HeadlessEngine,
  /// Retention off
  pub off: HeadlessEngine,
  compared: usize,
  kept: usize,
  reused: usize,
}

impl PixelPair {
  /// The two engines `make` builds, retention on in one and off in the
  /// other; `None` without a GPU
  pub fn new(make: impl Fn() -> EngineBuilder) -> Option<Self> {
    Some(Self {
      on: HeadlessEngine::new_or_skip(make().ui_retain(true))?,
      off: HeadlessEngine::new_or_skip(make().ui_retain(false))?,
      compared: 0,
      kept: 0,
      reused: 0,
    })
  }

  /// Do `f` to both engines
  pub fn each(&mut self, mut f: impl FnMut(&mut HeadlessEngine)) {
    f(&mut self.on);
    f(&mut self.off);
  }

  /// Run `n` frames on both. After each, their frames must be the same
  /// pixels (on a difference both are saved as PNGs in the temp folder's
  /// `ryo2d-ui-retain`), and the engine without retention must have kept
  /// and reused nothing.
  pub fn frames(&mut self, n: u32, what: &str) {
    for _ in 0..n {
      let (on, off) = (self.on.step(), self.off.step());
      self.compared += 1;
      let frame = self.compared;
      match (on, off) {
        (Ok(StepOutcome::Running), Ok(StepOutcome::Running)) => {}
        other => panic!("{what}, frame {frame}: {other:?}"),
      }
      let (Ok(on), Ok(off)) = (self.on.frame(), self.off.frame()) else {
        panic!("{what}, frame {frame}: no frame presented");
      };
      if on.as_raw() != off.as_raw() {
        let differ = on.pixels().zip(off.pixels()).filter(|(a, b)| a != b).count();
        let first = on
          .enumerate_pixels()
          .zip(off.pixels())
          .find(|((_, _, a), b)| a != b)
          .map(|((x, y, _), _)| (x, y));
        let folder = std::env::temp_dir().join("ryo2d-ui-retain");
        let name: String = what
          .chars()
          .map(|c| if c.is_alphanumeric() { c } else { '-' })
          .collect();
        let saved = std::fs::create_dir_all(&folder).is_ok()
          && on.save(folder.join(format!("{name}-{frame}-on.png"))).is_ok()
          && off.save(folder.join(format!("{name}-{frame}-off.png"))).is_ok();
        let saved = if saved {
          format!(" (both saved in {})", folder.display())
        } else {
          String::new()
        };
        panic!("{what}, frame {frame}: retention on and off differ in {differ} pixels, the first at {first:?}{saved}");
      }
      let off = self.off.ui_render_stats();
      assert!(
        !off.kept && off.texts_reused == 0,
        "{what}, frame {frame}: retention off kept or reused: {off:?}"
      );
      let on = self.on.ui_render_stats();
      self.kept += usize::from(on.kept);
      self.reused += on.texts_reused as usize;
    }
  }

  /// Frames compared so far
  pub fn compared(&self) -> usize {
    self.compared
  }

  /// Frames the retaining engine kept its UI image in, so far
  pub fn kept(&self) -> usize {
    self.kept
  }

  /// Texts the retaining engine reused, so far
  pub fn reused(&self) -> usize {
    self.reused
  }
}
```

`src/core/testing/mod.rs`: declare `mod retain;` gated as `headless` is, and export `pub use retain::PixelPair;` and `pub use crate::core::render::UiRenderStats;`.

`src/core/testing/headless.rs`, in `impl HeadlessEngine` next to `ui_cache_stats`:

```rust
  /// What the UI's render step did in the last frame: whether it kept the
  /// UI image, the texts it built and reused
  #[doc(hidden)]
  pub fn ui_render_stats(&self) -> UiRenderStats {
    self.engine.ui_render_stats()
  }

  /// The last frame's render step on the CPU (building, uploading and
  /// encoding the frame; not the UI pass)
  #[doc(hidden)]
  pub fn render_time(&self) -> Duration {
    self.engine.render_time()
  }
```

- [ ] **Step 2: Write the failing engine tests**

Create `tests/e2e/ui_retain.rs`, and register it in `tests/e2e.rs` as the other modules are (`#[path = "e2e/ui_retain.rs"] mod ui_retain;`):

```rust
//! UI render retention in the engine: with it on and off, two engines driven
//! alike draw the same pixels every frame; an unchanged UI keeps its image,
//! and a changing one rebuilds only its changed texts

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::PixelPair;

/// Ten panels with labels, and a frame counter when `counting`
struct Hud {
  counting: bool,
  frame: u32,
}

impl Scene for Hud {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    for i in 0..10 {
      let y = 8.0 + i as f32 * 20.0;
      ui.draw_rect(Rect::new(4.0, y - 2.0, 150.0, 18.0), Color::new(20, 20, 30, 200));
      ui.draw_text(&format!("Label {i}"), 8.0, y, Color::WHITE);
    }
    if self.counting {
      self.frame += 1;
      ui.draw_text(&format!("Frame {}", self.frame), 200.0, 8.0, Color::WHITE);
    }
    SceneAction::Continue
  }
}

fn hud(counting: bool) -> impl Fn() -> EngineBuilder {
  move || {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Hud { counting, frame: 0 })
  }
}

fn lock<T>(shared: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  shared
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn an_unchanged_ui_keeps_its_image() {
  let Some(mut pair) = PixelPair::new(hud(false)) else {
    return;
  };
  pair.frames(10, "the HUD opening");
  let before = pair.kept();
  pair.frames(10, "the HUD unchanged");
  assert_eq!(pair.kept() - before, 10, "every unchanged frame kept the image");
}

#[test]
fn a_changing_label_redraws_every_frame() {
  let Some(mut pair) = PixelPair::new(hud(true)) else {
    return;
  };
  pair.frames(5, "counting");
  for _ in 0..10 {
    pair.frames(1, "counting");
    assert!(!pair.on.ui_render_stats().kept);
  }
}

#[test]
fn a_resize_redraws_the_ui() {
  let Some(mut pair) = PixelPair::new(hud(false)) else {
    return;
  };
  pair.frames(10, "the HUD");
  assert!(pair.on.ui_render_stats().kept);
  pair.each(|engine| engine.resize(400, 300));
  pair.frames(1, "resized");
  assert!(!pair.on.ui_render_stats().kept, "a new UI image is drawn");
  pair.frames(10, "resized, unchanged");
  assert!(pair.on.ui_render_stats().kept);
}

/// A panel and a label while the shared flag is set, nothing otherwise
struct Toggle(Arc<Mutex<bool>>);

impl Scene for Toggle {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if *lock(&self.0) {
      ui.draw_rect(Rect::new(10.0, 10.0, 120.0, 40.0), Color::new(40, 60, 90, 255));
      ui.draw_text("Shown", 20.0, 20.0, Color::WHITE);
    }
    SceneAction::Continue
  }
}

#[test]
fn the_ui_hidden_then_shown_comes_back() {
  let shown = Arc::new(Mutex::new(true));
  let make = {
    let shown = shown.clone();
    move || {
      EngineBuilder::new()
        .resolution(320, 240)
        .start_scene(Toggle(shown.clone()))
    }
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(10, "shown");
  *lock(&shown) = false;
  pair.frames(5, "hidden");
  *lock(&shown) = true;
  pair.frames(5, "shown again");
  assert!(pair.on.ui_render_stats().kept);
}

#[test]
fn a_disabled_ui_pass_hides_the_kept_ui() {
  let Some(mut pair) = PixelPair::new(hud(false)) else {
    return;
  };
  pair.frames(10, "the HUD");
  let set_ui_pass = |pair: &mut PixelPair, on: bool| {
    pair.each(|engine| {
      engine
        .world_mut()
        .resource_mut::<RenderPipeline>()
        .set_enabled(pass_ids::UI, on)
        .expect("the ui pass exists");
    });
  };
  set_ui_pass(&mut pair, false);
  pair.frames(5, "the ui pass off");
  set_ui_pass(&mut pair, true);
  pair.frames(5, "the ui pass on again");
}
```

Import `RenderPipeline` and `pass_ids` the way `tests/e2e/render_pipeline.rs` does.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test --test e2e ui_retain`
Expected: compile errors: no method `ui_retain` on `EngineBuilder`, no `ui_render_stats`.

- [ ] **Step 4: The switch**

`src/core/config/debug.rs`, in `DebugConfig` after `ui_cache`:

```rust
  /// Whether the renderer keeps the UI image while the UI is unchanged, and
  /// reuses the glyphs built for unchanged texts (default true). Off, it
  /// builds and draws the whole UI every frame: the same pixels, slower.
  /// For ruling retention out when chasing a rendering bug.
  pub ui_retain: bool,
```

`ui_retain: true` in `Default`; a doc-table row after `ui_cache`'s:

```
//! | `ui_retain` | Keep the UI image while the UI is unchanged; reuse unchanged texts' glyphs (on by default; `EngineBuilder::ui_retain`) |
```

If `DebugConfig` is read from `Ryo.toml`, the field follows `ui_cache`'s handling there.

`src/core/engine/mod.rs`, after `EngineBuilder::ui_cache`:

```rust
  /// Whether the renderer keeps the UI image while the UI is unchanged and
  /// reuses the glyphs of unchanged texts (default true). Off, the whole UI
  /// is built and drawn every frame: the same pixels, slower. For ruling
  /// retention out when chasing a rendering bug.
  pub fn ui_retain(mut self, on: bool) -> Self {
    self.config.debug.ui_retain = on;
    self
  }
```

- [ ] **Step 5: The UI image's epoch**

`src/core/graphics/compositor.rs`: a field `ui_epoch: u64` beside `ui_texture`, set to `next_ui_epoch()` where the compositor is created and again in `resize_ui`:

```rust
/// An epoch no UI image has had in this process: a recreated compositor's
/// image never matches an old one's
fn next_ui_epoch() -> u64 {
  static NEXT: AtomicU64 = AtomicU64::new(1);
  NEXT.fetch_add(1, Ordering::Relaxed)
}
```

```rust
  /// Changes whenever the UI image is created anew (it is then blank)
  pub(crate) fn ui_epoch(&self) -> u64 {
    self.ui_epoch
  }
```

- [ ] **Step 6: A kept frame composites the image without the `ui` pass**

`src/core/render/frame.rs`, in `FrameScene` after `ui_size`:

```rust
  /// The UI image already holds this frame's UI (it was kept): the `ui`
  /// pass does not run, and the composite blends the image as it is
  pub(crate) ui_kept: bool,
```

`src/core/graphics/renderer.rs`: a field `ui_drawn: bool` (initialised `false`), and:

```rust
  /// The UI image's epoch (`Compositor::ui_epoch`)
  pub(crate) fn ui_image_epoch(&self) -> u64 {
    self.compositor.ui_epoch()
  }

  /// Whether the last frame blended the UI image (drawn by its `ui` pass,
  /// or kept)
  pub(crate) fn ui_drawn(&self) -> bool {
    self.ui_drawn
  }
```

In `render_frame`: set `self.ui_drawn = false;` first, before `self.target.acquire()?`. Read `let ui_kept = scene.ui_kept;` before `scene` moves into the context. After `RenderContext::new(..)`:

```rust
      // A kept UI image is blended as it is, unless the game turned the
      // `ui` pass off
      ctx.ui_drawn = ui_kept && pipeline.is_enabled(pass_ids::UI) == Some(true);
```

After `pipeline.execute(&mut ctx);`, beside `captured = ctx.captured;`, keep `ui_drawn = ctx.ui_drawn;` (a local declared beside `captured`), and assign `self.ui_drawn = ui_drawn;` after the submit. `render_blank_frame` passes `ui_kept: false`.

`RenderContext::ui_drawn`'s doc (`src/core/render/context.rs:50`) becomes "Whether the frame's UI image is blended: drawn by the `ui` pass this frame, or kept from an earlier one".

- [ ] **Step 7: The engine keeps or draws**

`src/core/engine/mod.rs`: an engine field `ui_retain: crate::core::render::ui_retain::UiRetain` after `ui_camera` (comment: "UI render retention: what the UI image holds"), initialised with `Default::default()`; and:

```rust
  /// What the UI's render step did in the last frame
  pub(crate) fn ui_render_stats(&self) -> UiRenderStats {
    self.ui_retain.stats
  }

  /// The last frame's render step on the CPU (building, uploading and
  /// encoding; not the UI pass)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn render_time(&self) -> Duration {
    self.frame_timings.render + self.frame_timings.present
  }
```

`src/core/engine/extract.rs`, the UI section. After `text_targets` is made, replace the `self.ui_camera = emit_ui(...)` statement with:

```rust
      // An unchanged UI keeps the UI image: none of it is built, uploaded
      // or drawn (render module notes, "UI render retention")
      let retain = self.config.debug.ui_retain;
      if !retain {
        self.ui_retain.clear();
      }
      let env = UiRenderEnv {
        space: self.ui_space,
        on_target: ui_on_target,
        text: sources.is_some(),
        fonts: self.text.generation(),
      };
      let icons = sources.as_ref().map(|sources| sources.icons);
      let ui_kept =
        retain && self.ui_retain.keep(&self.ui_commands, icons, env, rend.ui_image_epoch());
      self.ui_camera = if ui_kept {
        None
      } else {
        emit_ui(
          &self.ui_commands,
          &self.ui_space,
          ui_on_target,
          sources.as_ref(),
          &mut text_targets,
          triangles,
          real_time,
        )
      };
      self.ui_retain.stats = UiRenderStats {
        kept: ui_kept,
        glyph_evictions: gpu.glyph_evictions(),
        ..UiRenderStats::default()
      };
```

The block computing `(entity_sprites, lights)` returns `(entity_sprites, lights, ui_kept)`; `FrameScene` gets `ui_kept`. Before `let outcome = match result {`:

```rust
    // What the UI image holds now
    match &result {
      Ok(()) => self
        .ui_retain
        .presented(ui_kept, rend.ui_drawn(), rend.ui_image_epoch()),
      Err(_) => self.ui_retain.lost(),
    }
```

Remove Task 2's three temporary `allow(dead_code)` attributes, and Task 1's from the `glyph_evictions` methods used here.

- [ ] **Step 8: Run the tests**

Run: `cargo test --test e2e ui_retain`
Expected: PASS (5 tests). If `an_unchanged_ui_keeps_its_image` keeps fewer than 10 frames, something in the bare scene's UI changes every frame: find what (print the differing command with a temporary `eprintln!` in `keep`), and fix the cause, never the test.

Run: `cargo test -p ryo2d --lib render::ui_retain`
Expected: PASS.

- [ ] **Step 9: Run the gate and commit**

```bash
git add src/core/graphics/compositor.rs src/core/graphics/renderer.rs src/core/render/frame.rs src/core/render/context.rs src/core/render/ui_retain/mod.rs src/core/engine/extract.rs src/core/engine/mod.rs src/core/config/debug.rs src/core/testing/retain.rs src/core/testing/headless.rs src/core/testing/mod.rs src/core/graphics/sprite_gpu.rs src/core/graphics/atlas.rs src/core/graphics/glyph_atlas.rs tests/e2e/ui_retain.rs tests/e2e.rs
git commit -m "feat: an unchanged UI keeps its UI image" -m "- the renderer builds, uploads and draws no UI when the frame's commands match what the UI image holds
- the composite blends the kept image; a new image, an unpresented frame or a disabled ui pass are honoured
- EngineBuilder::ui_retain and DebugConfig::ui_retain turn it off
- PixelPair compares retention on and off pixel for pixel"
```

(Stage only the files you changed of those listed.)

---

### Task 4: Changing UIs reuse their unchanged texts' glyphs

**Files:**
- Modify: `src/core/render/text.rs` (`emit_text` at `:115-144`)
- Modify: `src/core/render/draw_list.rs` (`DrawList` at `:185-351`)
- Modify: `src/core/render/frame.rs` (`PixelRect` at `:102-104`: add `Hash`)
- Create: `src/core/render/ui_retain/texts.rs`
- Modify: `src/core/render/ui_retain/mod.rs` (`UiRetain::texts`)
- Modify: `src/core/render/ui.rs` (`emit_ui` at `:164-384`)
- Modify: `src/core/engine/extract.rs` (the UI section)
- Modify: `tests/e2e/ui_retain.rs`

**Interfaces:**
- Consumes: Task 1's `SpriteGpuRenderer::{glyph_page_generation, pin_glyph_page, glyphs_refused}`; Task 2's `UiRenderEnv`, `TextLayout::has_effects`; Task 3's `UiRetain`, stats wiring, `PixelPair`.
- Produces:
  - `pub(crate) fn emit_glyphs(sources: &TextSources<'_>, targets: &mut TextTargets<'_>, layout: &TextLayout, place: &TextPlacement)` and `pub(crate) fn emit_icons(..same..)`; `emit_text` calls both
  - `DrawList::glyph_count(&self) -> usize`, `DrawList::glyphs_from(&self, start: usize) -> &[DrawGlyph]`, `DrawList::push_glyphs(&mut self, kind: DrawKind, z: i32, lit: bool, only_camera: Option<Entity>, glyphs: &[DrawGlyph])`
  - `pub(crate) struct UiTextCache` (`Default`) with `begin(&mut self, env: UiRenderEnv, reuse: bool)`, `emit(&mut self, sources: &TextSources<'_>, targets: &mut TextTargets<'_>, layout: &Arc<TextLayout>, place: &TextPlacement)`, `end(&mut self)`, `clear(&mut self)`, `built(&self) -> u32`, `reused(&self) -> u32`
  - `UiRetain::texts: UiTextCache` (`pub(crate)`); `UiRetain::clear` clears it too
  - `emit_ui(commands: &UiCommands, env: &UiRenderEnv, sources: Option<&TextSources<'_>>, targets: &mut TextTargets<'_>, triangles: &mut Triangles, real_time: f32, texts: &mut UiTextCache) -> Option<CameraDraw>`

- [ ] **Step 1: Write the failing tests**

In `src/core/render/draw_list.rs`'s tests module (create one if there is none):

```rust
  #[test]
  fn pushed_glyphs_keep_their_order_at_one_depth() {
    let glyph = |x: f32| DrawGlyph {
      page: GlyphPage {
        format: GlyphFormat::Mask,
        index: 0,
      },
      instance: SpriteInstance::new().with_transform(x, 0.0, 0.0, 1.0, 1.0),
      shader: BatchShader::TextMask,
      blend: BlendMode::Alpha,
      sampling: Sampling::Linear,
      layers: RenderLayers::all(),
      min: Vec2::ZERO,
      max: Vec2::ONE,
      scissor: None,
    };
    let mut list = DrawList::default();
    list.push_glyphs(DrawKind::Sprite, 5, false, None, &[glyph(1.0), glyph(2.0), glyph(3.0)]);
    assert_eq!(list.glyph_count(), 3);
    assert_eq!(list.glyphs_from(1).len(), 2);
    list.sorted();
    let order: Vec<_> = list
      .unlit_items()
      .iter()
      .map(|item| (item.z, item.payload))
      .collect();
    assert_eq!(
      order,
      vec![
        (5, DrawPayload::Glyph(0)),
        (5, DrawPayload::Glyph(1)),
        (5, DrawPayload::Glyph(2)),
      ]
    );
  }
```

(Use the real names of the shader, sampling and unlit-items accessor; the compiler names them. The assertion stays.)

Append to `tests/e2e/ui_retain.rs`:

```rust
#[test]
fn one_changed_label_builds_one_text() {
  let Some(mut pair) = PixelPair::new(hud(true)) else {
    return;
  };
  pair.frames(5, "counting");
  for frame in 0..10 {
    pair.frames(1, "counting");
    let on = pair.on.ui_render_stats();
    assert_eq!(
      (on.kept, on.texts_built, on.texts_reused),
      (false, 1, 10),
      "frame {frame}: {on:?}"
    );
    let off = pair.off.ui_render_stats();
    assert_eq!(
      (off.texts_built, off.texts_reused),
      (11, 0),
      "retention off builds every text"
    );
  }
}

/// A waving greeting and three still labels
struct Wavy;

impl Scene for Wavy {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_text_styled(
      RichText::markup("[wave]Hello[/wave]"),
      8.0,
      8.0,
      &TextStyle::default().size(24.0),
    );
    for i in 0..3 {
      ui.draw_text(&format!("Still {i}"), 8.0, 60.0 + i as f32 * 20.0, Color::WHITE);
    }
    SceneAction::Continue
  }
}

#[test]
fn an_effect_text_is_built_every_frame() {
  let make = || EngineBuilder::new().resolution(320, 240).start_scene(Wavy);
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(5, "waving");
  for frame in 0..10 {
    pair.frames(1, "waving");
    let on = pair.on.ui_render_stats();
    assert_eq!(
      (on.kept, on.texts_built, on.texts_reused),
      (false, 1, 3),
      "frame {frame}: {on:?}"
    );
  }
}

#[test]
fn a_new_ui_space_rebuilds_every_text() {
  let Some(mut pair) = PixelPair::new(hud(true)) else {
    return;
  };
  pair.frames(5, "counting");
  pair.each(|engine| engine.resize(400, 300));
  pair.frames(1, "resized");
  let on = pair.on.ui_render_stats();
  assert_eq!((on.texts_built, on.texts_reused), (11, 0), "{on:?}");
  pair.frames(1, "resized, counting");
  assert_eq!(pair.on.ui_render_stats().texts_reused, 10);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib render::draw_list` and `cargo test --test e2e ui_retain`
Expected: compile error (no `push_glyphs`); then, once it compiles, the e2e count tests fail on `texts_built` (0 before this task).

- [ ] **Step 3: Split `emit_text`**

`src/core/render/text.rs`:

```rust
/// Lay a block's glyphs and icons into the draw list
pub(crate) fn emit_text(
  sources: &TextSources<'_>,
  targets: &mut TextTargets<'_>,
  layout: &TextLayout,
  place: &TextPlacement,
) {
  emit_glyphs(sources, targets, layout, place);
  emit_icons(sources, targets, layout, place);
}

/// Lay a block's glyphs into the draw list: its shadows, outlines, then
/// fills (a bitmap font's cells are sprites)
pub(crate) fn emit_glyphs(
  sources: &TextSources<'_>,
  targets: &mut TextTargets<'_>,
  layout: &TextLayout,
  place: &TextPlacement,
) {
  if !(place.px_per_layout.is_finite() && place.px_per_layout > 0.0) {
    return;
  }
  let mut block = Block {
    sources,
    targets,
    layout,
    place,
  };
  let mut effects = EffectRun::new(place);
  for layer in [Layer::Shadow, Layer::Outline, Layer::Fill] {
    for (index, glyph) in layout.glyphs.iter().enumerate() {
      let at = Glyph {
        index: u32::try_from(index).unwrap_or(u32::MAX),
        glyph,
      };
      emit_glyph(&mut block, at, layer, &mut effects);
    }
  }
}

/// Lay a block's inline icons into the draw list
pub(crate) fn emit_icons(
  sources: &TextSources<'_>,
  targets: &mut TextTargets<'_>,
  layout: &TextLayout,
  place: &TextPlacement,
) {
  if !(place.px_per_layout.is_finite() && place.px_per_layout > 0.0) {
    return;
  }
  let mut block = Block {
    sources,
    targets,
    layout,
    place,
  };
  for icon in &layout.icons {
    emit_icon(&mut block, icon);
  }
}
```

- [ ] **Step 4: The draw list**

`src/core/render/frame.rs`: `PixelRect` derives `Hash` too.

`src/core/render/draw_list.rs`, in `impl DrawList`:

```rust
  /// How many glyphs the frame holds
  pub(crate) fn glyph_count(&self) -> usize {
    self.glyphs.len()
  }

  /// The glyphs pushed from the `start`-th on, in the order pushed
  pub(crate) fn glyphs_from(&self, start: usize) -> &[DrawGlyph] {
    self.glyphs.get(start..).unwrap_or(&[])
  }

  /// Add `glyphs` at `z`, in their order, for `only_camera` or every camera
  /// (a text's glyphs built in an earlier frame)
  pub(crate) fn push_glyphs(
    &mut self,
    kind: DrawKind,
    z: i32,
    lit: bool,
    only_camera: Option<Entity>,
    glyphs: &[DrawGlyph],
  ) {
    let start = self.glyphs.len();
    self.glyphs.extend_from_slice(glyphs);
    for index in start..self.glyphs.len() {
      self.push_item(lit, z, kind, DrawPayload::Glyph(index), only_camera);
    }
  }
```

- [ ] **Step 5: The text cache**

Create `src/core/render/ui_retain/texts.rs`:

```rust
//! A UI text's glyphs, built once and drawn again while the text, its
//! placement and the glyph pages they sample are unchanged

use std::sync::Arc;

use super::UiRenderEnv;
use crate::core::graphics::GlyphPage;
use crate::core::hash::FastMap;
use crate::core::render::PixelRect;
use crate::core::render::draw_list::DrawGlyph;
use crate::core::render::text::{TextPlacement, TextSources, TextTargets, emit_glyphs, emit_icons, emit_text};
use crate::core::text::TextLayout;

/// Glyph buffers kept for reuse when their texts go
const SPARE: usize = 64;

/// What a text's glyphs were built from, besides the UI's environment: its
/// layout (by address, held by the entry) and its placement
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TextKey {
  layout: usize,
  origin: [u32; 2],
  reveal: Option<[u32; 2]>,
  opacity: u32,
  seed: u64,
  scissor: Option<PixelRect>,
}

impl TextKey {
  fn of(layout: &Arc<TextLayout>, place: &TextPlacement) -> TextKey {
    TextKey {
      layout: Arc::as_ptr(layout) as usize,
      origin: place.origin.to_array().map(f32::to_bits),
      reveal: place.reveal.map(|(shown, fade)| [shown.to_bits(), fade.to_bits()]),
      opacity: place.opacity.to_bits(),
      seed: place.seed,
      scissor: place.draw.scissor,
    }
  }
}

/// A text's built glyphs
#[derive(Debug)]
struct Built {
  /// Held so its address, in the key, is not reused while the entry lives
  _layout: Arc<TextLayout>,
  glyphs: Vec<DrawGlyph>,
  /// Each glyph page the glyphs sample, with its generation when they were
  /// built: they are good while every one is unchanged
  pages: Vec<(GlyphPage, u64)>,
  /// The last frame it was drawn in
  used: u64,
}

/// The UI's texts' built glyphs, kept from one frame to the next
#[derive(Debug, Default)]
pub(crate) struct UiTextCache {
  built: FastMap<TextKey, Built>,
  spare: Vec<Vec<DrawGlyph>>,
  env: Option<UiRenderEnv>,
  /// Keep and reuse built glyphs (retention is on)
  reuse: bool,
  frame: u64,
  texts_built: u32,
  texts_reused: u32,
}

impl UiTextCache {
  /// Start a frame drawn in `env`. Glyphs built in another environment are
  /// dropped, and so is everything when `reuse` is off.
  pub(crate) fn begin(&mut self, env: UiRenderEnv, reuse: bool) {
    if !reuse || self.env != Some(env) {
      self.clear();
    }
    self.env = Some(env);
    self.reuse = reuse;
    self.frame += 1;
    self.texts_built = 0;
    self.texts_reused = 0;
  }

  /// Draw a text: the glyphs built for it earlier when it has them and
  /// their pages still hold them (pinning the pages for this frame), else
  /// newly built ones, kept for the next frames. Its icons are drawn anew
  /// each time; a text with an effect is built every frame.
  pub(crate) fn emit(
    &mut self,
    sources: &TextSources<'_>,
    targets: &mut TextTargets<'_>,
    layout: &Arc<TextLayout>,
    place: &TextPlacement,
  ) {
    if !self.reuse || layout.has_effects() {
      emit_text(sources, targets, layout, place);
      self.texts_built += 1;
      return;
    }
    let key = TextKey::of(layout, place);
    if let Some(built) = self.built.get_mut(&key)
      && built
        .pages
        .iter()
        .all(|&(page, generation)| targets.sprites.glyph_page_generation(page) == Some(generation))
    {
      for &(page, _) in &built.pages {
        targets.sprites.pin_glyph_page(page);
      }
      built.used = self.frame;
      let draw = place.draw;
      targets
        .list
        .push_glyphs(draw.kind, draw.z, draw.lit, draw.only_camera, &built.glyphs);
      self.texts_reused += 1;
    } else {
      let (glyphs, sprites) = (targets.list.glyph_count(), targets.list.sprite_count());
      let refused = targets.sprites.glyphs_refused();
      emit_glyphs(sources, targets, layout, place);
      self.texts_built += 1;
      // Kept only when every glyph was placed (a full atlas refuses some
      // for this frame) and none is a bitmap font's sprite cell
      let complete = targets.sprites.glyphs_refused() == refused
        && targets.list.sprite_count() == sprites;
      if complete {
        self.keep(key, layout, targets, glyphs);
      }
    }
    emit_icons(sources, targets, layout, place);
  }

  /// Keep the glyphs pushed from the `start`-th on as `key`'s
  fn keep(&mut self, key: TextKey, layout: &Arc<TextLayout>, targets: &TextTargets<'_>, start: usize) {
    let mut glyphs = self.spare.pop().unwrap_or_default();
    glyphs.clear();
    glyphs.extend_from_slice(targets.list.glyphs_from(start));
    let mut pages: Vec<(GlyphPage, u64)> = Vec::new();
    for glyph in &glyphs {
      if pages.iter().any(|&(page, _)| page == glyph.page) {
        continue;
      }
      match targets.sprites.glyph_page_generation(glyph.page) {
        Some(generation) => pages.push((glyph.page, generation)),
        None => return,
      }
    }
    let built = Built {
      _layout: layout.clone(),
      glyphs,
      pages,
      used: self.frame,
    };
    if let Some(old) = self.built.insert(key, built) {
      self.recycle(old.glyphs);
    }
  }

  fn recycle(&mut self, glyphs: Vec<DrawGlyph>) {
    if self.spare.len() < SPARE {
      self.spare.push(glyphs);
    }
  }

  /// End the frame: texts not drawn in it are dropped
  pub(crate) fn end(&mut self) {
    let frame = self.frame;
    let mut gone = Vec::new();
    self.built.retain(|_, built| {
      let drawn = built.used == frame;
      if !drawn {
        gone.push(std::mem::take(&mut built.glyphs));
      }
      drawn
    });
    for glyphs in gone {
      self.recycle(glyphs);
    }
  }

  /// Drop everything built
  pub(crate) fn clear(&mut self) {
    self.built.clear();
    self.env = None;
  }

  /// Texts built in the frame
  pub(crate) fn built(&self) -> u32 {
    self.texts_built
  }

  /// Texts drawn from glyphs built earlier, in the frame
  pub(crate) fn reused(&self) -> u32 {
    self.texts_reused
  }
}
```

Fix import paths to where the types live (the compiler names them); make `TextPlacement`'s fields and `TextDraw`'s readable here if they are not (`pub(crate)`). `end` collects into `gone` to avoid borrowing `self` inside `retain`; if it shows in the profile, reuse a scratch `Vec` field instead.

`src/core/render/ui_retain/mod.rs`: `mod texts; pub(crate) use texts::UiTextCache;`, a field `pub(crate) texts: UiTextCache` on `UiRetain` ("The texts' built glyphs"), and `self.texts.clear();` in `UiRetain::clear`.

- [ ] **Step 6: `emit_ui` draws texts through the cache**

`src/core/render/ui.rs`: `emit_ui`'s `space: &UiSpace, pixel_art: bool` become `env: &UiRenderEnv`, and it takes `texts: &mut UiTextCache` last; the body binds `let (space, pixel_art) = (&env.space, env.on_target);` first. The `UiCommand::Text` arm ends with:

```rust
        texts.emit(sources, targets, layout, &place);
```

in place of `emit_text(sources, targets, layout, &place);`. Update the doc comment's parameter mentions.

`src/core/engine/extract.rs`, the drawing branch:

```rust
      self.ui_camera = if ui_kept {
        None
      } else {
        self.ui_retain.texts.begin(env, retain);
        let camera = emit_ui(
          &self.ui_commands,
          &env,
          sources.as_ref(),
          &mut text_targets,
          triangles,
          real_time,
          &mut self.ui_retain.texts,
        );
        self.ui_retain.texts.end();
        camera
      };
      let (texts_built, texts_reused) = if ui_kept {
        (0, 0)
      } else {
        (self.ui_retain.texts.built(), self.ui_retain.texts.reused())
      };
      self.ui_retain.stats = UiRenderStats {
        kept: ui_kept,
        texts_built,
        texts_reused,
        glyph_evictions: gpu.glyph_evictions(),
      };
```

Remove Task 1's remaining temporary `allow(dead_code)` attributes.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p ryo2d --lib render` and `cargo test --test e2e ui_retain`
Expected: PASS. If `one_changed_label_builds_one_text` reuses fewer than 10, the still labels' layouts are not the same `Arc` from frame to frame: find where `draw_text` makes its layout (the text cache's `layout_keyed`) and report it; do not loosen the assertion. If `a_new_ui_space_rebuilds_every_text` still reuses after the resize, check whether `HeadlessEngine::resize` changed the `UiSpace` (print it): if the space is unchanged, the reuse is correct and the test needs a change that does alter the space (a resize to a different aspect, or a scale factor); say which in a comment.

- [ ] **Step 8: Run the gate and commit**

```bash
git add src/core/render/text.rs src/core/render/draw_list.rs src/core/render/frame.rs src/core/render/ui_retain/texts.rs src/core/render/ui_retain/mod.rs src/core/render/ui.rs src/core/engine/extract.rs src/core/graphics/sprite_gpu.rs src/core/graphics/atlas.rs src/core/graphics/glyph_atlas.rs tests/e2e/ui_retain.rs
git commit -m "feat: changing UIs reuse their unchanged texts' glyphs" -m "- a text drawn again with the same layout and placement pushes the glyphs built for it, pinning their glyph pages
- glyphs whose page was cleared since are built again
- text effects, bitmap-font cells and texts a full atlas cut short are built every frame"
```

(Stage only the files you changed of those listed.)

---

### Task 5: The hard cases match pixel for pixel

**Files:**
- Modify: `tests/e2e/ui_retain.rs`
- Create: `src/core/render/ui_retain/gpu_tests.rs`; Modify: `src/core/render/ui_retain/mod.rs` (`#[cfg(test)] mod gpu_tests;`)

**Interfaces:**
- Consumes: `PixelPair` and `UiRenderStats` (Task 3), the retention of Tasks 3 and 4; `ryo2d::testing::fixtures::settings_tree() -> UiNode`; `SceneContext::spawn_ui`; `HeadlessEngine::{ui_focusables, mouse_move, press_mouse, release_mouse, wheel, tap_key, resize, world_mut}`; `UiTheme::set(Theme)`, `Theme::light()`; `TextIcons::insert(name, SpriteHandle)`; `Fonts::role(FontRole::Ui) -> Option<FontId>`, `Fonts::set_render(FontId, FontRender)`, `FontRender::msdf()`; `Text2d::new(..).with_style(TextStyle::new().size(..))`.
- Produces: tests only.

Each test below should pass as soon as it is written, since Tasks 3 and 4 implemented the behaviour; a failure is a retention bug to find and fix with superpowers:systematic-debugging (the saved PNGs show where). For each test, before relying on it, prove it can fail: temporarily break what it guards (e.g. return `0` from `stamp_of` for the icons test, skip the page-generation check for the eviction tests, drop `on_target` from `UiRenderEnv`'s comparison for the pixel-art test), watch it fail, restore. Note each in your report.

- [ ] **Step 1: The node settings screen: hover, scrolling, a dropdown, a theme change**

Append to `tests/e2e/ui_retain.rs`:

```rust
/// The 200-widget settings node screen
struct SettingsScreen;

impl Scene for SettingsScreen {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(ryo2d::testing::fixtures::settings_tree());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn settings() -> EngineBuilder {
  EngineBuilder::new()
    .resolution(1280, 720)
    .start_scene(SettingsScreen)
}

/// The centre of the `n`-th widget the pad can reach
fn widget(pair: &PixelPair, n: usize) -> (f32, f32) {
  let targets = pair.on.ui_focusables();
  let (_, rect) = targets[n % targets.len()];
  (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

#[test]
fn hover_scroll_a_dropdown_and_a_theme_change_match() {
  let Some(mut pair) = PixelPair::new(settings) else {
    return;
  };
  pair.frames(10, "the screen opening");
  for n in [1, 4, 7] {
    let (x, y) = widget(&pair, n);
    pair.each(|engine| engine.mouse_move(x, y));
    pair.frames(3, "hovering");
  }
  pair.each(|engine| engine.wheel(0.0, -3.0));
  pair.frames(20, "scrolling down");
  // A dropdown opened by its trigger, then closed
  let (x, y) = widget(&pair, 3);
  pair.each(|engine| engine.mouse_move(x, y));
  pair.frames(2, "over the dropdown");
  pair.each(|engine| engine.press_mouse(MouseButton::Left));
  pair.frames(2, "pressing");
  pair.each(|engine| engine.release_mouse(MouseButton::Left));
  pair.frames(5, "the dropdown open");
  pair.each(|engine| engine.tap_key(KeyCode::Escape));
  pair.frames(5, "the dropdown closed");
  pair.each(|engine| engine.world_mut().resource_mut::<UiTheme>().set(Theme::light()));
  pair.frames(10, "the light theme");
  assert!(pair.kept() > 0, "an unchanged frame kept the image");
  assert!(pair.reused() > 0, "a changing frame reused texts");
}
```

If index 3 is not the dropdown in `settings_tree()`'s focus order, pick the index that is (print `ui_focusables()` once) and say so in a comment.

- [ ] **Step 2: Swapped text icons show at once**

```rust
/// A prompt with a named icon, and a still label
struct Prompt;

impl Scene for Prompt {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_text_styled(
      RichText::markup("Press [icon=confirm] to start"),
      8.0,
      8.0,
      &TextStyle::default().size(24.0),
    );
    ui.draw_text("Still", 8.0, 60.0, Color::WHITE);
    SceneAction::Continue
  }
}

#[test]
fn swapped_text_icons_show_at_once() {
  let swatch = |r, g, b, name| {
    SpriteHandle::from_sprite(Sprite::solid_rect(16, 16, r, g, b, 255), name)
  };
  let (red, blue) = (swatch(255, 0, 0, "red"), swatch(0, 0, 255, "blue"));
  let make = || EngineBuilder::new().resolution(320, 240).start_scene(Prompt);
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.each(|engine| {
    engine
      .world_mut()
      .resource_mut::<TextIcons>()
      .insert("confirm", red.clone());
  });
  pair.frames(10, "the red icon");
  assert!(pair.on.ui_render_stats().kept);
  pair.each(|engine| {
    engine
      .world_mut()
      .resource_mut::<TextIcons>()
      .insert("confirm", blue.clone());
  });
  pair.frames(1, "the icon swapped");
  assert!(!pair.on.ui_render_stats().kept, "the swap redraws the UI");
  pair.frames(5, "the blue icon");
}
```

- [ ] **Step 3: A font render change, a UI alternating every frame, pixel art**

```rust
#[test]
fn a_font_render_change_matches() {
  let Some(mut pair) = PixelPair::new(hud(true)) else {
    return;
  };
  pair.frames(10, "raster text");
  pair.each(|engine| {
    let mut fonts = engine.world_mut().resource_mut::<Fonts>();
    let ui = fonts.role(FontRole::Ui).expect("a UI font");
    fonts.set_render(ui, FontRender::msdf());
  });
  pair.frames(10, "MSDF text");
}

/// Still labels, and one that alternates between two words every frame
struct Blink {
  frame: u32,
}

impl Scene for Blink {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    self.frame += 1;
    for i in 0..5 {
      ui.draw_text(&format!("Still {i}"), 8.0, 8.0 + i as f32 * 20.0, Color::WHITE);
    }
    let word = if self.frame % 2 == 0 { "Tick" } else { "Tock" };
    ui.draw_text(word, 200.0, 8.0, Color::WHITE);
    SceneAction::Continue
  }
}

#[test]
fn a_ui_alternating_every_frame_matches() {
  let make = || {
    EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Blink { frame: 0 })
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(30, "ticking");
  assert!(pair.reused() > 0);
}

#[test]
fn a_pixel_art_ui_matches() {
  let make = || {
    EngineBuilder::new()
      .pixel_art(320, 180)
      .start_scene(Hud {
        counting: true,
        frame: 0,
      })
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(20, "a pixel-art HUD");
  assert!(pair.reused() > 0);
}
```

- [ ] **Step 4: Glyph pressure from the UI and from world text**

```rust
/// Enough characters that cycling them at large sizes fills the glyph pages
const CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// Eight characters of `CHARS` for frame `frame`, and a size cycling
/// through five large ones
fn churn(frame: u32) -> (String, f32) {
  let start = (frame as usize * 8) % CHARS.len();
  let text: String = CHARS.chars().cycle().skip(start).take(8).collect();
  (text, 150.0 + (frame / 8 % 5) as f32 * 50.0)
}

/// A still large title, and large text that changes every frame
struct UiChurn {
  frame: u32,
}

impl Scene for UiChurn {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    self.frame += 1;
    ui.draw_text_styled("Title", 8.0, 8.0, &TextStyle::default().size(120.0));
    let (text, size) = churn(self.frame);
    ui.draw_text_styled(text, 8.0, 200.0, &TextStyle::default().size(size));
    SceneAction::Continue
  }
}

#[test]
fn glyph_pressure_from_the_ui_matches() {
  let make = || {
    EngineBuilder::new()
      .resolution(640, 480)
      .start_scene(UiChurn { frame: 0 })
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(120, "churning");
  assert!(
    pair.on.ui_render_stats().glyph_evictions > 0,
    "the run evicted glyph pages"
  );
  assert!(pair.reused() > 0, "the title was reused");
}

/// A still UI whose label the test can change, over world text that changes
/// every frame
struct WorldChurn {
  label: Arc<Mutex<String>>,
  text: Option<Entity>,
  frame: u32,
}

impl Scene for WorldChurn {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.camera().set_position(0.0, 0.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.frame += 1;
    if let Some(text) = self.text.take() {
      ctx.world.despawn(text);
    }
    let (text, size) = churn(self.frame);
    let spawned = ctx.world.spawn((
      Transform::from_xy(0.0, 0.0),
      Text2d::new(text).with_style(TextStyle::new().size(size)),
    ));
    self.text = Some(spawned.id());
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    for i in 0..5 {
      ui.draw_text(&format!("Still {i}"), 8.0, 8.0 + i as f32 * 20.0, Color::WHITE);
    }
    let label = lock(&self.label).clone();
    ui.draw_text_styled(label, 8.0, 120.0, &TextStyle::default().size(48.0));
    SceneAction::Continue
  }
}

#[test]
fn world_text_churn_under_a_kept_ui_matches() {
  let label = Arc::new(Mutex::new("Before".to_string()));
  let make = {
    let label = label.clone();
    move || {
      EngineBuilder::new()
        .resolution(640, 480)
        .start_scene(WorldChurn {
          label: label.clone(),
          text: None,
          frame: 0,
        })
    }
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(120, "the UI still over churning world text");
  assert!(pair.kept() > 100, "the UI image was kept: {}", pair.kept());
  assert!(
    pair.on.ui_render_stats().glyph_evictions > 0,
    "the world text evicted glyph pages"
  );
  *lock(&label) = "After".to_string();
  pair.frames(10, "the UI label changed");
}
```

Adapt `ctx.world.spawn(..)` / `despawn` to the engine's world API (see `tests/e2e/text.rs`), keeping the behaviour. If either eviction assertion fails, the sizes or character counts do not fill eight 1024² pages: raise them (world text rasterises at most 256 px and snaps to size steps, so prefer more distinct characters there), and say so in a comment. If a pair differs while the atlas reports refusals (`glyphs_refused`), the churn overflowed a frame: reduce the per-frame glyphs.

- [ ] **Step 5: A sprite updated in place (needs crate access)**

Create `src/core/render/ui_retain/gpu_tests.rs` and declare `#[cfg(test)] mod gpu_tests;` in `ui_retain/mod.rs`:

```rust
//! Retention with what only the engine can do: a sprite's pixels replaced
//! in place, as hot reload does

use crate::core::asset::SpriteHandle;
use crate::core::engine::EngineBuilder;
use crate::core::graphics::Sprite;
use crate::core::testing::PixelPair;
use crate::prelude::*;

/// A sprite and a label
struct SpriteUi(SpriteHandle);

impl Scene for SpriteUi {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.draw_sprite(&self.0, 10.0, 10.0);
    ui.draw_text("Sprite", 40.0, 10.0, Color::WHITE);
    SceneAction::Continue
  }
}

#[test]
fn a_sprite_updated_in_place_redraws() {
  let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 255, 0, 0, 255), "swatch");
  let make = {
    let sprite = sprite.clone();
    move || {
      EngineBuilder::new()
        .resolution(160, 120)
        .start_scene(SpriteUi(sprite.clone()))
    }
  };
  let Some(mut pair) = PixelPair::new(make) else {
    return;
  };
  pair.frames(10, "red");
  assert!(pair.on.ui_render_stats().kept);
  sprite.update(Sprite::solid_rect(8, 8, 0, 0, 255, 255));
  pair.frames(1, "blue, the same size");
  assert!(!pair.on.ui_render_stats().kept, "the update redraws the UI");
  pair.frames(5, "blue");
  sprite.update(Sprite::solid_rect(16, 16, 0, 255, 0, 255));
  pair.frames(5, "green and larger");
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo test --test e2e ui_retain` and `cargo test -p ryo2d --lib render::ui_retain`
Expected: PASS, every new test also watched failing against its temporary break (Steps 1-5 note).

- [ ] **Step 7: Run the gate and commit**

```bash
git add tests/e2e/ui_retain.rs src/core/render/ui_retain/gpu_tests.rs src/core/render/ui_retain/mod.rs
git commit -m "test: UI render retention on and off match in the hard cases" -m "- hover, scrolling, a dropdown and a theme change on the settings node screen
- swapped text icons, a font render change, a UI alternating every frame, pixel art
- glyph pressure from UI and world text, a sprite updated in place"
```

(Add any retention fix the tests forced as its own `fix:` commit first, with the test that found it named in the message body.)

---

### Task 6: The ui demo and breakout under the oracle

**Files:**
- Create: `examples/demos/ui/tests/retain.rs`
- Create: `examples/games/breakout/tests/retain.rs`

**Interfaces:**
- Consumes: `PixelPair` (Task 3); `ui_demo::demo().start_page(page).engine_builder()`; breakout's `breakout::scenes::{GameScene, MenuScene}` and the `game_folder()` setup in `examples/games/breakout/tests/cache.rs:16-31` (copy it: tests in separate files cannot share it).
- Produces: tests only.

- [ ] **Step 1: Every page of the ui demo**

`examples/demos/ui/tests/retain.rs`:

```rust
//! Every page of the ui demo with UI render retention on and off: two
//! engines driven by the same input must draw the same pixels every frame

use demo_kit::prelude::*;
use ryo2d::testing::PixelPair;

/// The demo's pages (`ui_demo::demo`)
const PAGES: usize = 17;

/// Open page `page`, leave it still, move the focus, point at and scroll
/// over its widgets, press one and back out; the frames compared, kept and
/// the texts reused
fn run(page: usize) -> Option<(usize, usize, usize)> {
  let mut pair = PixelPair::new(|| ui_demo::demo().start_page(page).engine_builder())?;
  let what = |step: &str| format!("page {page}, {step}");
  pair.frames(20, &what("opening"));
  pair.frames(10, &what("still"));
  for key in [KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::ArrowRight] {
    pair.each(|engine| engine.tap_key(key));
    pair.frames(3, &what("moving the focus"));
  }
  let targets = pair.on.ui_focusables();
  for (_, rect) in targets.iter().take(3) {
    let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    pair.each(|engine| engine.mouse_move(x, y));
    pair.frames(3, &what("pointing"));
  }
  pair.each(|engine| engine.wheel(0.0, -3.0));
  pair.frames(15, &what("scrolling"));
  pair.each(|engine| engine.tap_key(KeyCode::Enter));
  pair.frames(5, &what("pressing"));
  pair.each(|engine| engine.tap_key(KeyCode::Escape));
  pair.frames(10, &what("backing out"));
  Some((pair.compared(), pair.kept(), pair.reused()))
}

#[test]
fn every_page_looks_the_same_retained() {
  let (mut compared, mut kept, mut reused) = (0, 0, 0);
  for page in 0..PAGES {
    let Some((c, k, r)) = run(page) else {
      return;
    };
    eprintln!("page {page}: {c} frames compared, {k} kept, {r} texts reused");
    (compared, kept, reused) = (compared + c, kept + k, reused + r);
  }
  assert!(compared > 0);
  assert!(kept > 0, "some unchanged frame kept the UI image");
  assert!(reused > 0, "some changing frame reused texts");
}
```

If a key in the script makes a page quit or leave the demo, replace it with a harmless one for that page and note it in a comment. The spec asks for a modal opening and closing: check in `examples/demos/ui/src/scenes/clipping_demo.rs` what opens its modal; if `Enter` on the focused widget does not, add that page's own step (its key, or a click on its button) to `run` for page 6, followed by `Escape`.

The `text` demo is a binary with no library, so a test cannot start its scenes: its effect pages are covered by the tour comparison (Task 8, Step 4) and by `an_effect_text_is_built_every_frame` (Task 4).

- [ ] **Step 2: Breakout's menus, pause menu and HUD during play**

`examples/games/breakout/tests/retain.rs`: copy `game_folder()` from `examples/games/breakout/tests/cache.rs:16-31` (change the folder name's prefix to `breakout-retain-`), then:

```rust
//! Breakout's menus and HUD with UI render retention on and off: two engines
//! driven by the same input must draw the same pixels every frame

use std::path::PathBuf;
use std::sync::OnceLock;

use breakout::scenes::{GameScene, MenuScene};
use ryo2d::prelude::*;
use ryo2d::testing::PixelPair;

// game_folder() here, copied as described

fn pair<S: Scene + 'static>(start: fn() -> S) -> Option<PixelPair> {
  game_folder();
  PixelPair::new(move || {
    EngineBuilder::new()
      .resolution(1280, 720)
      .fixed_hz(120)
      .start_scene(start())
  })
}

#[test]
fn the_main_menu_looks_the_same_retained() {
  let Some(mut pair) = pair(MenuScene::new) else {
    return;
  };
  pair.frames(30, "the menu opening");
  for key in [KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::ArrowUp] {
    pair.each(|engine| engine.tap_key(key));
    pair.frames(5, "moving through the menu");
  }
  pair.frames(30, "the menu still");
  assert!(pair.kept() > 0);
}

#[test]
fn the_pause_menu_and_the_hud_look_the_same_retained() {
  let Some(mut pair) = pair(|| GameScene::new(0, None)) else {
    return;
  };
  pair.each(|engine| engine.tap_key(KeyCode::Space));
  pair.frames(30, "the ball launched");
  for _ in 0..3 {
    pair.each(|engine| engine.press_key(KeyCode::KeyA));
    pair.frames(40, "the paddle left");
    pair.each(|engine| engine.release_key(KeyCode::KeyA));
    pair.each(|engine| engine.press_key(KeyCode::KeyD));
    pair.frames(60, "the paddle right");
    pair.each(|engine| engine.release_key(KeyCode::KeyD));
  }
  pair.each(|engine| engine.tap_key(KeyCode::Escape));
  pair.frames(20, "paused");
  pair.each(|engine| engine.tap_key(KeyCode::ArrowDown));
  pair.frames(10, "paused, moving");
  pair.each(|engine| engine.tap_key(KeyCode::Escape));
  pair.frames(30, "playing again");
  assert!(pair.kept() > 0, "the paused menu kept the image");
  assert!(pair.reused() > 0, "the HUD reused its still texts");
}
```

Use breakout's real constructors (`MenuScene::new` may take arguments; follow `cache.rs`).

- [ ] **Step 3: Run the tests**

Run: `cargo test -p ui-demo --test retain` (the demo's package name, from its `Cargo.toml`) and `cargo test -p breakout --test retain`
Expected: PASS. A difference names the page or step and saves both PNGs: debug it with superpowers:systematic-debugging, fix the cause in a `fix:` commit naming this test, and rerun.

- [ ] **Step 4: Run the gate and commit**

```bash
git add examples/demos/ui/tests/retain.rs examples/games/breakout/tests/retain.rs
git commit -m "test: the ui demo and breakout match with UI render retention on and off"
```

---

### Task 7: `ui:retain`, the overlay's render line and the docs

**Files:**
- Modify: `src/core/console/builtins.rs` (help `:108-124`, registration `:346-363`, tests `:656+`)
- Modify: `src/core/ui/debug.rs` (`RebuildsOverlay` `:56-128`)
- Modify: `src/core/engine/frame.rs` (the overlay call `:636-646`)
- Modify: `.claude/skills/ryo-ui/SKILL.md` + `src/cli/skills/ryo-ui.md`; `.claude/skills/ryo-api/SKILL.md` + `src/cli/skills/ryo-api.md`; `.claude/skills/ryo-console/SKILL.md` + `src/cli/skills/ryo-console.md`
- Modify: `src/core/render/AGENTS.md`, `docs/specs/2026-10-19-ui-render-retention-design.md`

**Interfaces:**
- Consumes: `DebugConfig::ui_retain` (Task 3), `UiRenderStats` (Task 2), `Engine::ui_render_stats` (Task 3).
- Produces: console `ui:retain [on|off]`; `RebuildsOverlay::draw(&mut self, ui: &mut UiContext, world: &World, dt: f32, render: UiRenderStats)`; `RenderTally` (private to `ui/debug.rs`) with `add(&mut self, stats: UiRenderStats, dt: f32)` and `line(&self) -> &str`.

- [ ] **Step 1: Write the failing tests**

`src/core/console/builtins.rs` tests, beside `ui_cache_turns_caching_off_and_on`:

```rust
  #[test]
  fn ui_retain_turns_retention_off_and_on() {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);
    let mut console = super::super::ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);
    assert!(ctx.debug_config.ui_retain, "on by default");
    assert_eq!(
      registry.execute("ui:retain off", &mut ctx),
      CommandResult::Ok
    );
    assert!(!ctx.debug_config.ui_retain);
    assert_eq!(registry.execute("ui:retain", &mut ctx), CommandResult::Ok);
    assert!(!ctx.debug_config.ui_retain, "alone it only prints");
    registry.execute("ui:retain on", &mut ctx);
    assert!(ctx.debug_config.ui_retain);
    assert!(matches!(
      registry.execute("ui:retain maybe", &mut ctx),
      CommandResult::Error(_)
    ));
  }
```

`src/core/ui/debug.rs` tests (create the module if there is none):

```rust
  #[test]
  fn the_render_line_totals_a_second() {
    let mut tally = RenderTally::default();
    let kept = UiRenderStats {
      kept: true,
      ..UiRenderStats::default()
    };
    let drawn = UiRenderStats {
      texts_built: 2,
      texts_reused: 30,
      ..UiRenderStats::default()
    };
    assert_eq!(tally.line(), "UI render: measuring");
    for _ in 0..3 {
      tally.add(kept, 0.25);
    }
    tally.add(drawn, 0.0); // a rerun pass: not counted
    assert_eq!(tally.line(), "UI render: measuring");
    tally.add(drawn, 0.25);
    assert_eq!(
      tally.line(),
      "UI image kept 3 of 4 frames, texts built 2 / reused 30"
    );
    tally.add(kept, 0.25);
    assert_eq!(
      tally.line(),
      "UI image kept 3 of 4 frames, texts built 2 / reused 30",
      "shown once a second"
    );
  }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ryo2d --lib console::builtins` and `cargo test -p ryo2d --lib ui::debug`
Expected: FAIL: unknown command `ui:retain`; no `RenderTally`.

- [ ] **Step 3: The console command**

Help, after the `ui:cache` line:

```rust
        ctx
          .console
          .print("  ui:retain [on|off]  - Show or set UI render retention");
```

Registration, after `ui:cache`'s:

```rust
  // UI render retention
  registry.register(
    "ui:retain",
    "Show or set UI render retention: ui:retain [on|off]",
    |args, ctx| {
      match args.first().map(|arg| arg.to_ascii_lowercase()).as_deref() {
        None => {}
        Some("on") => ctx.debug_config.ui_retain = true,
        Some("off") => ctx.debug_config.ui_retain = false,
        Some(_) => return CommandResult::Error("Usage: ui:retain [on|off]".to_string()),
      }
      let state = if ctx.debug_config.ui_retain {
        "on"
      } else {
        "off"
      };
      ctx.console.print(format!("UI render retention is {state}"));
      CommandResult::Ok
    },
  );
```

- [ ] **Step 4: The overlay's render line**

`src/core/ui/debug.rs`:

```rust
/// The render step's totals over a second, for the overlay's render line.
/// The line changes once a second, so while the UI is still the overlay
/// redraws it only then.
#[derive(Debug, Default)]
struct RenderTally {
  seconds: f32,
  frames: u32,
  kept: u32,
  built: u32,
  reused: u32,
  line: Option<String>,
}

impl RenderTally {
  /// Count a frame's `stats`; `dt` is zero for a pass that reruns the
  /// frame, which is not counted
  fn add(&mut self, stats: UiRenderStats, dt: f32) {
    if dt <= 0.0 {
      return;
    }
    self.seconds += dt;
    self.frames += 1;
    self.kept += u32::from(stats.kept);
    self.built += stats.texts_built;
    self.reused += stats.texts_reused;
    if self.seconds >= 1.0 {
      self.line = Some(format!(
        "UI image kept {} of {} frames, texts built {} / reused {}",
        self.kept, self.frames, self.built, self.reused
      ));
      *self = RenderTally {
        line: self.line.take(),
        ..RenderTally::default()
      };
    }
  }

  fn line(&self) -> &str {
    self.line.as_deref().unwrap_or("UI render: measuring")
  }
}
```

`RebuildsOverlay` gains a field `render: RenderTally` and `draw` a last parameter `render: UiRenderStats`; at its start `self.render.add(render, dt);`, and after the counter text is drawn, the render line below it, right-aligned the same way:

```rust
      let line = self.render.line();
      let width = ui.measure_text(line, &style).x;
      ui.draw_text_styled(line, screen as f32 - width - 4.0, 4.0 + LABEL_SIZE * 2.0, &style);
```

(Adjust the line spacing to the style's line height if `LABEL_SIZE * 2.0` overlaps.) Update `draw`'s doc to mention the render line, and the struct doc.

`src/core/engine/frame.rs`: `self.ui_rebuilds.draw(&mut ui, &self.world, dt, self.ui_retain.stats);`

`src/core/config/debug.rs`: `show_ui_rebuilds`'s doc gains "and a render line (the UI image kept, texts built and reused, over the last second)".

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ryo2d --lib console::builtins` and `cargo test -p ryo2d --lib ui::debug`
Expected: PASS.

- [ ] **Step 6: The docs**

- **`ryo-ui` skill** (both copies, then `cmp`; bump the version): beside the node-cache paragraph (`ui:cache off`, around `src/cli/skills/ryo-ui.md:1301`), a paragraph:
  > **Rendering an unchanged UI is nearly free.** When a frame's UI draws exactly what the last frame's did, the renderer keeps the UI image and draws no UI; a changing UI rebuilds only its changed texts, reusing the rest's glyphs. Both UI styles get it automatically. A text with an effect (`[wave]`, `[shake]`, `[rainbow]`, or a game's own) redraws the UI every frame while it is on screen, as does anything that changes every frame (a ticking clock, a blinking caret); a text in a bitmap font is rebuilt each time the UI redraws. `ui:retain off` (console) or `EngineBuilder::ui_retain(false)` draws the whole UI every frame: the same pixels, slower, to rule retention in or out when chasing a rendering bug. `debug:ui-rebuilds` shows, once a second, how many frames kept the UI image and how many texts were built and reused.
- **`ryo-api` skill** (both copies; bump): the builder table row after `ui_cache(bool)`: `| ui_retain(bool) | whether the renderer keeps the UI image while the UI is unchanged and reuses unchanged texts' glyphs (default true); false draws the whole UI every frame, the same pixels slower, to rule retention out of a rendering bug (also DebugConfig::ui_retain, the console's ui:retain on\|off) |`, escaped as the neighbouring rows are.
- **`ryo-console` skill** (both copies; bump): a `ui:retain [on|off]` row beside `ui:cache`, and the `debug:ui-rebuilds` row mentions the render line.
- **`src/core/render/AGENTS.md`**: a section "UI render retention" after the UI bullets, with each decision and its reason:
  - An unchanged UI keeps the UI image: the composite blends it as it is, so the frame builds, uploads and draws no UI (the UI is composited over the world, so the world changing does not matter).
  - What "unchanged" means: the same commands in the same order (shared layouts and points by address, each held by the retainer so an address is not reused under it; numbers by bits), the same sprite and icon versions (stamps: a sprite's generation; a text's icons, names resolved through `TextIcons`), no text effect, the same `UiRenderEnv` (UI space, pixel-art snapping, text on, the text engine's font generation), and the UI image's epoch (process-unique; a recreated image is blank).
  - The image record is committed only for a presented frame whose `ui` pass drew; a kept frame with the `ui` pass disabled shows no UI and leaves the image as it is.
  - A changing frame reuses texts' built glyphs by `TextKey` (layout address, origin, reveal, opacity, seed, scissor) within one environment; only texts are cached (92% of building the UI's draw items in 4g's profile); rects, sprites and lines are built every frame (cheaper than a lookup). Effects, bitmap-font cells (sprites, re-resolved each frame) and texts a full atlas cut short are not kept.
  - Glyph pages carry process-unique generations, changed when a page is cleared: a reused text whose page changed is built again. A reused text pins its pages for the frame (counted in the eviction test as "drawn this frame").
  - UI draws must not read time on the GPU; a UI draw that does must count as time-driven.
  - Update the existing bullet "The UI is composited only if the `ui` pass drew this frame" to say "drew or kept this frame".
- **The spec** (`docs/specs/2026-10-19-ui-render-retention-design.md`): in "Incremental build", add after the first paragraph: "Only texts are cached: building them is 92% of building the UI's draw items, and a rect, sprite or line costs less to build than to look up. A text drawn in a bitmap font (its cells are sprites) or cut short by a full glyph atlas is built every frame." In "Exposure and tooling", the `debug:ui-rebuilds` bullet becomes: "`debug:ui-rebuilds` gains a render line, updated once a second (so the overlay itself redraws the UI only then): the frames the UI image was kept, and the texts built and reused."

- [ ] **Step 7: Run the gate and commit**

```bash
cmp .claude/skills/ryo-ui/SKILL.md src/cli/skills/ryo-ui.md
cmp .claude/skills/ryo-api/SKILL.md src/cli/skills/ryo-api.md
cmp .claude/skills/ryo-console/SKILL.md src/cli/skills/ryo-console.md
git add src/core/console/builtins.rs src/core/ui/debug.rs src/core/engine/frame.rs src/core/config/debug.rs .claude/skills/ryo-ui/SKILL.md src/cli/skills/ryo-ui.md .claude/skills/ryo-api/SKILL.md src/cli/skills/ryo-api.md .claude/skills/ryo-console/SKILL.md src/cli/skills/ryo-console.md src/core/render/AGENTS.md docs/specs/2026-10-19-ui-render-retention-design.md
git commit -m "feat: ui:retain and a render line in debug:ui-rebuilds" -m "- the console turns UI render retention off and on
- the rebuilds overlay shows, once a second, the frames that kept the UI image and the texts built and reused
- the skills, the render module notes and the spec describe retention"
```

---

### Task 8: Benches, the guard, the tour and the measurement

**Files:**
- Create: `benches/ui_render.rs`; Modify: `Cargo.toml` (`[[bench]]`), `benches/text.rs`
- Modify: `docs/backlog/ui-retained-nodes.md`

**Interfaces:**
- Consumes: `HeadlessEngine::{render_time, ui_render_stats}` (Task 3), `EngineBuilder::ui_retain` (Task 3), `ryo2d::testing::fixtures::settings_tree`.
- Produces: the `ui_render` bench and its guard; the 4g section of the backlog.

- [ ] **Step 1: The bench and its guard**

`Cargo.toml`, after the `ui` bench:

```toml
[[bench]]
name = "ui_render"
harness = false
```

`benches/ui_render.rs`:

```rust
//! The UI's render-side cost with a GPU (`HeadlessEngine`, 1280x720): the
//! 200-widget settings node screen unchanged, with a label changing every
//! frame, and scrolling, with UI render retention on and off, and an empty
//! frame. Before criterion runs, a guard checks 4g's targets on medians of
//! the render step's CPU time (not the UI pass).

use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;
use ryo2d::testing::fixtures::settings_tree;

/// Frames each median is taken over
const FRAMES: usize = 2000;

struct Empty;

impl Scene for Empty {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// The settings screen; with `label`, a counter over it changes every frame
struct Settings {
  label: bool,
  frame: u32,
}

impl Scene for Settings {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(settings_tree());
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if self.label {
      self.frame += 1;
      ui.draw_text(&format!("Frame {}", self.frame), 8.0, 8.0, Color::WHITE);
    }
    SceneAction::Continue
  }
}

fn settings(label: bool) -> Settings {
  Settings { label, frame: 0 }
}

/// An engine on `scene`, warmed up
fn engine(scene: impl Scene + 'static, retain: bool) -> Option<HeadlessEngine> {
  let builder = EngineBuilder::new()
    .resolution(1280, 720)
    .ui_retain(retain)
    .start_scene(scene);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(60).ok()?;
  Some(engine)
}

/// The median render step over `FRAMES` frames
fn median(engine: &mut HeadlessEngine) -> Duration {
  let mut times: Vec<Duration> = (0..FRAMES)
    .map(|_| {
      let _ = engine.step();
      engine.render_time()
    })
    .collect();
  times.sort();
  times[times.len() / 2]
}

/// 4g's targets: an idle settings screen adds under 5 µs of render-side
/// work over an empty frame, one changing label under 60 µs, and a
/// changing frame is no slower retained than not
fn guard() {
  let (Some(mut empty), Some(mut idle), Some(mut idle_off), Some(mut label), Some(mut label_off)) = (
    engine(Empty, true),
    engine(settings(false), true),
    engine(settings(false), false),
    engine(settings(true), true),
    engine(settings(true), false),
  ) else {
    return;
  };
  let empty = median(&mut empty);
  let over = |engine: &mut HeadlessEngine| median(engine).saturating_sub(empty);
  let (idle, idle_off, label, label_off) = (
    over(&mut idle),
    over(&mut idle_off),
    over(&mut label),
    over(&mut label_off),
  );
  eprintln!(
    "ui_render guard, render step over an empty frame ({empty:?}): idle {idle:?} (retention off {idle_off:?}), one label {label:?} (off {label_off:?})"
  );
  assert!(idle < Duration::from_micros(5), "an idle UI costs {idle:?} to render");
  assert!(label < Duration::from_micros(60), "one changing label costs {label:?}");
  assert!(label <= label_off, "retained {label:?}, not retained {label_off:?}");
}

fn bench_ui_render(c: &mut Criterion) {
  guard();
  for (name, label, retain) in [
    ("settings_idle", false, true),
    ("settings_idle_unretained", false, false),
    ("settings_label", true, true),
    ("settings_label_unretained", true, false),
  ] {
    let Some(mut engine) = engine(settings(label), retain) else {
      return;
    };
    c.bench_function(name, |b| b.iter(|| engine.step()));
  }
  for (name, retain) in [("settings_scrolling", true), ("settings_scrolling_unretained", false)] {
    let Some(mut engine) = engine(settings(false), retain) else {
      return;
    };
    engine.mouse_move(640.0, 360.0);
    let mut frame = 0u32;
    c.bench_function(name, |b| {
      b.iter(|| {
        frame += 1;
        // Down for a second, up for a second: the list keeps moving
        let dy = if frame / 120 % 2 == 0 { -1.0 } else { 1.0 };
        engine.wheel(0.0, dy);
        engine.step()
      })
    });
  }
  let Some(mut engine) = engine(Empty, true) else {
    return;
  };
  c.bench_function("empty_frame", |b| b.iter(|| engine.step()));
}

criterion_group!(benches, bench_ui_render);
criterion_main!(benches);
```

`benches/text.rs`: copy the HUD bench as `hud_frame_unretained`, its builder with `.ui_retain(false)`, registered beside `hud_frame`.

- [ ] **Step 2: Run the benches**

Run: `cargo bench --bench ui_render 2>&1 | tee /private/tmp/claude-501/4g-bench-ui-render.txt | tail -40` and `cargo bench --bench text 2>&1 | tee /private/tmp/claude-501/4g-bench-text.txt | tail -30`
Expected: the guard line printed and passing; criterion numbers for every bench. If a guard assertion fails, do not change its threshold: profile the frame it names (Step 3) and report the numbers and the profile's top entries.

- [ ] **Step 3: Profile the changing frame**

Profile `settings_label` (retained) the way the 4g research did (`/private/tmp/claude-501/4g-research/ui-render-path.md`, §2 Method: a release build with `debug = 1`, `xctrace record --template 'Time Profiler'`, inclusive time per function divided by frames). Delete the traces afterwards. Record per frame: `emit_ui`, `UiRetain::keep`, `UiTextCache::emit` (reused vs built), `Batch2D::prepare`, `write_buffer`, `encode_render_pass`.

- [ ] **Step 4: The demo tour against the baseline**

```bash
./dev.sh demos:tour
diff -rq /private/tmp/claude-501/4g-tour/base-1 target/demo-tour > /private/tmp/claude-501/4g-tour/after.txt || true
grep -vxF -f /private/tmp/claude-501/4g-tour/nondeterministic.txt /private/tmp/claude-501/4g-tour/after.txt || echo "no differences beyond the nondeterministic pages"
```

Expected: "no differences beyond the nondeterministic pages". A page differing beyond them is a retention bug: find it with superpowers:systematic-debugging, fix it in a `fix:` commit with a test, and rerun.

- [ ] **Step 5: Write the measurement into the backlog**

Add a section "4g: UI render retention (measured)" to `docs/backlog/ui-retained-nodes.md`:
- the guard's medians (idle, one label, retention on and off) against the research's figures (render side about 120 µs; HUD frame 248.3 µs);
- the HUD bench before (Task 1's report) and after, with and without retention;
- `settings_scrolling` retained against unretained (a scrolling list rebuilds its visible texts each frame; reuse by translation is the possible follow-up);
- the changing frame's profile from Step 3;
- **Approach C** (uploading only changed instance ranges; recording the UI's draws once as a render bundle): whether it is worth a sub-project, from how much of the changing frame `Batch2D::prepare`, the uploads and encoding now take; and 4h (layout and resolve retention for node trees, ceiling about 18 µs of the UI pass) beside it. The owner decides.

- [ ] **Step 6: Run the gate and commit**

```bash
git add benches/ui_render.rs benches/text.rs Cargo.toml docs/backlog/ui-retained-nodes.md
git commit -m "test: UI render benches with a guard, and the 4g measurement" -m "- the settings node screen idle, with a changing label and scrolling, retained and not, against an empty frame
- the guard checks the idle and changing-frame targets on render-step medians
- the backlog records the numbers and whether approach C is worth it"
```

(If `Cargo.lock` changed, it should not have: no new dependencies.)
