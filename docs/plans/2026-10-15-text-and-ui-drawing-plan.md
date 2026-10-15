# Text and UI Drawing (4a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the CPU-rasterized UI and its 8×8 ASCII bitmap font with:
- real fonts (Parley layout, swash raster glyphs, per-font MSDF via `fdsm`);
- world-space text on entities;
- a UI drawn on the GPU through the world's 2D batch.

The immediate-mode `Scene::ui` call style stays.

**Architecture:**
- **Text core.** A GPU-free text core (`src/core/text/`) owns the fonts, styles, rich text, markup, layout (cached), effects and glyph rasterization.
- **Renderer.** The renderer owns a glyph atlas beside the sprite atlas. Glyphs become instanced batch quads with new `BatchShader` entries.
  - World text (`Text2d`) joins the draw list, z-sorted with sprites per camera.
  - The UI records `UiCommand`s that the extract step lays into the same draw list for a UI camera. The `ui` pass draws those onto the display image (or, in pixel-art mode with window-resolution UI, onto a UI image that the composite blends premultiplied).

**Tech Stack:**
- parley 0.12 (`default-features = false, features = ["std"]`)
- swash 0.2.10
- fdsm 0.8
- etagere 0.2 (already a dependency)
- wgpu 30
- bevy_ecs 0.20 (as `ryo2d::ecs`)
- criterion (benches)

**Spec:** `docs/specs/2026-10-15-text-and-ui-drawing-design.md` (commit 94ab3c5). This plan corrects it where its Plan Notes say so, in the same commit as the plan.

**Research behind this plan** (session scratchpad; the implementer may read them, nothing in them is binding):
- `text-spike/REPORT.md`: the exact Parley/swash/fdsm/etagere APIs, timings, sizes and fonts;
- `plan4a-batch.md`: batch, draw list, atlas, cameras, passes, tests;
- `plan4a-ui.md`: `UiContext`, engine UI pass, console, overlays, dev windows, demo-kit, the examples inventory.

All are under `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/`.

## Global Constraints

- **Crate boundaries:**
  - `parley`, `swash` and `fdsm` are named only in `Cargo.toml` and `src/core/text/`. The renderer receives glyph bitmaps (`GlyphImage`), never a font type.
  - `rapier2d` only in `Cargo.toml` and `src/core/physics/`.
  - `bevy_ecs` only in `Cargo.toml`, `src/core/ecs/kernel.rs` and the hidden prelude re-export.
- **No panics in library code:** no `unwrap`, `expect` or `panic!`. Font and text errors are `Result`s or warnings (`engine_warn!`). Tests may unwrap.
- **Warnings:** invalid input is warned once per font and character, once per style, or once per entity, never every frame.
- **Bind groups:** every one goes through `graphics::bind_groups::create` and is made with its resource, never per frame (`steady_frames_create_no_bind_groups` stays green and gains a text frame).
- **Built-in fonts:** the cargo feature `default-fonts` is on by default. It embeds Inter Regular and Bold, JetBrains Mono Regular and m5x7, subset to Latin, Latin Extended, Greek and Cyrillic (m5x7 is ASCII-only), with their licence files.
- **Units:**
  - UI coordinates are UI units. With the automatic scale these equal today's screen units, so existing `draw_rect`/`draw_text` coordinates keep their meaning.
  - World text sizes are world units.
- **Colour:** every batch shader writes colour premultiplied by alpha.
  - `Color` tints are sRGB, straight alpha, decoded in the shader, as sprites do.
  - The UI draws after tonemapping and post-processing, into its own image, blended in gamma space (Plan Notes).
- **Rich components:** `#[non_exhaustive]` with public fields and builders.
- **Skills:** copies in `.claude/skills/*/SKILL.md` and `src/cli/skills/*.md` stay byte-identical (`cmp`).
- **Line endings:** LF everywhere.
- **Commit messages:**
  - `type: brief description` plus bullets;
  - no co-author or attribution lines;
  - never mention AGENTS.md.
- **Scope:** networking is out of scope beyond compiling and keeping leap-king and the networking demo working. Breaking changes are acceptable.
- **The gate, at the end of every task:**
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`
  - `cargo test --workspace`

## Review Focus

1. **Characters no loaded font has** (CJK in the dropdown demo, emoji, a private-use codepoint): expected the fallback chain is tried, then the font's tofu box is drawn. It is warned once per font and character, layout continues, and nothing panics. Tests: Task 2 (`a_character_no_font_has_lays_out_as_tofu_and_warns_once`) and Task 7 (`a_missing_glyph_draws_a_box`).
2. **Glyphs and frames bigger than the atlas** (a 3000 px glyph; 10,000 distinct glyphs in one frame): expected a glyph larger than a page is skipped with one warning, and a frame needing more than every page holds draws what fits, warns once, and keeps every glyph drawn this frame. Tests: Task 5 (`a_glyph_larger_than_a_page_is_refused_once`, `a_full_atlas_keeps_this_frames_glyphs`).
3. **Degenerate sizes and scales** (text size 0, negative or NaN; wrap width 0 or NaN; `ui_scale` 0 or NaN; an entity scaled to 0): expected nothing drawn for that text, one warning per style or entity, no NaN reaching the GPU, and the rest of the frame unaffected. Tests: Task 2 (`degenerate_sizes_lay_out_nothing`), Task 7 (`a_text_entity_scaled_to_zero_draws_nothing`), Task 8 (`a_bad_ui_scale_falls_back_to_automatic`).
4. **The UI drawn twice in one frame** (a scene switch returned from `ui` reruns the pass): expected the second pass replaces the first pass's commands, with no doubled translucent panels. Test: Task 8 (`a_scene_switched_from_ui_draws_its_ui_the_same_frame`, rewritten to check one draw).
5. **A font registered after text using it is on screen** (a font loaded in `on_enter` of a later scene, or by a later `ctx.fonts().load`): expected the text lays out again with the new font on the next frame (the generation bump), with no stale cached layout or glyphs. Tests: Task 2 (`a_font_registration_invalidates_cached_layouts`), Task 7 (`world_text_relays_out_when_its_font_arrives`).

## Plan Notes (decisions the plan makes against the spec)

The spec is corrected to match these in the same commit as this plan.

- **Font loading.** It is `ctx.fonts()` (a `Mut<Fonts>`) with `Fonts::load(path) -> AssetResult<FontId>` and `Fonts::add_bytes`, not `ctx.assets.load_font`. `AssetManager` is two path-keyed maps (sprites, shaders) with no generic loader, and fonts get their own registry until the asset rewrite (sub-project 5). On wasm, `Fonts::load` returns `AssetError::LoadError` (no file system); games embed fonts with `add_bytes(include_bytes!(..))`.
- **The glyph atlas** lives at `src/core/graphics/glyph_atlas.rs`, owned by `SpriteGpuRenderer` beside the sprite atlas (which owns the two samplers and the page bind-group lookup the batch draws through), not at `render/batch/glyphs.rs`.
- **The UI image.** The UI draws into a UI image, and the composite blends it over the frame in gamma (sRGB) space.
  - **Format.** The image is `Rgba8Unorm` holding gamma-encoded premultiplied colour (the `UI_GAMMA` pipeline-override constant makes every batch shader encode its output). The composite shader decodes the frame, blends `ui + frame × (1 − ui.a)` in gamma space, and encodes the result.
  - **Why gamma space.** Alpha in the UI behaves the way designers expect, the way the web, egui and the old CPU UI did: a 50% black panel over white is 128. Linear blending would make a 67%-opaque dark backing over white come out about 157 instead of about 85. Text anti-aliasing also keeps its usual weight.
  - **Size.** The image is the render size by default: window resolution in the default presentation, the low-res target in pixel-art mode, so it scales with the art. With `UiResolution::Window` in pixel-art mode, it is the window's physical screen rectangle.
  - **Versus the spec.** The spec said "onto the output after the composite". The result is the same (window resolution, after tonemap and post), and the composite keeps the capture path unchanged.
- **Shorthand signatures stay.** `draw_text(text, x, y, color)`, `draw_text_anchored`, `draw_text_centered`, `draw_text_wrapped*`, `label`, `text_size -> (u32, u32)` and `wrap_text(max_width: u32)` keep their signatures. They draw or measure with the default UI style (`FontRole::Ui`, 14 px). `draw_text_styled` and `ui.text(..)` take a `TextStyle`. So every `ui` hook compiles unchanged, and the example port is visual tuning, not a rewrite.
- **`ui_scale`** becomes `Option<f32>`: render pixels per UI unit, where `None` is automatic. The automatic value keeps UI units equal to screen units: the scale factor by default, the whole-pixel scale for window-resolution UI in pixel-art mode, and 1 on the low-res target. The engine default changes from `Some(1)` to `None`. A `Some(s)` now scales rect coordinates as well as text (before, only glyphs scaled), and mouse positions are converted to match.
- **`finish_dropdowns`** becomes crate-internal. The engine runs it after each scene's `ui`, drawing open lists on the popup layer.
- **Markup tags.** The style tags are `color`, `size`, `font`, `b`, `i`, `outline`, `icon` and `pause`. Any other tag, `[name]` or `[name=args]`, is an effect tag. The parser cannot know which effects a game registered at runtime, so an effect name nobody registered is warned once when the text is drawn and the text draws plain. Only malformed markup renders literally (and is warned once per markup string): an unclosed `[`, a close with no open, an unclosed tag, or a bad value for a style tag. The spec said "unknown tag renders literally", which would make every custom effect tag literal.
- **Bitmap fonts** (`Fonts::add_bitmap`) are laid out by a small monospace layouter. A text uses either a bitmap font or outline fonts; a span asking for the other kind is warned once and falls back to the text's base font.
- **Defaults:**
  - UI text size is 14 px.
  - MSDF defaults are base 32 px, range 4 px.
  - Outline and glow widths on MSDF fonts are limited to half the range at the base size, scaled with the text. A game that wants wider effects raises the range.
- **The spike.** The spec's first migration step was run while this plan was written, outside the repo (session scratchpad `text-spike/`). Parley 0.12 and swash 0.2.10 agree on glyph ids; strokes, hinting, variable fonts and synthesized italic work; fdsm makes MSDFs with sharp corners in 231-337 µs a glyph; all three build for wasm32. The code in Tasks 1-7 uses its working calls, so no task repeats it. The wasm clippy in every task's gate keeps the web build honest.
- **Binary size.** The spike measured parley + swash + fdsm at about 2.0 MB of code (size-optimised, stripped) plus about 0.35 MB of fonts, on native and on wasm. Task 1 records the engine's own numbers. Trimming (parley features, ICU data) is a follow-up, not part of 4a.

## File Structure

**New, `src/core/text/`** (`parley`/`swash`/`fdsm` appear only here):

| File | Responsibility |
|---|---|
| `mod.rs` | module doc, re-exports |
| `fonts.rs` | `Fonts` (resource), `FontId`, `FontRender`, `FontRole`, `FontError`, the built-in fonts, bitmap fonts |
| `style.rs` | `TextStyle`, `TextFont`, `Outline`, `Shadow`, `Glow`, `TextLayoutOptions`, `TextAlign`, `TextOverflow`, style hashing |
| `rich.rs` | `RichText`, `Span`, `Icon`, the marks (spans, icons, pauses, effects) |
| `markup.rs` | `RichText::markup` parser |
| `layout.rs` | `TextLayout`, `PlacedGlyph`, `PlacedIcon`, `LineInfo`, `GlyphStyle`; the Parley layouter and the bitmap layouter |
| `engine.rs` | `TextEngine` (crate-internal, engine-owned): font sync from `Fonts`, Parley contexts, the layout cache, measuring, caret and hit-testing |
| `effects.rs` | `TextEffects` (resource), `GlyphContext`, `GlyphMod`, the built-in effects, reveal evaluation |
| `icons.rs` | `TextIcons` (resource) |
| `raster.rs` | `GlyphKey`, `GlyphImage`, `GlyphRasterizer` (swash raster, stroke, pixel threshold, synthesis), `raster_size_step` |
| `msdf.rs` | swash outline into an `fdsm` shape, MSDF generation |
| `world.rs` | `Text2d`, `TextBounds`, `TextReveal`, `TextRevealed`, crate-internal `TextLayoutState`, `advance_text_reveal` |
| `cpu.rs` | `CpuText`: monospace glyph masks for the dev windows' CPU buffers |
| `AGENTS.md` | module conventions |

**New elsewhere:**
- `src/assets/fonts/`: the four subset fonts, `OFL-Inter.txt`, `OFL-JetBrainsMono.txt`, `m5x7-LICENSE.txt` (CC0), `README.md` (sources and the subset command).
- `src/core/graphics/glyph_atlas.rs`: `GlyphAtlas`, `GlyphPage`, `GlyphSlot`.
- `src/core/render/text.rs`: glyph quads from a `TextLayout` (shared by world text and the UI).
- `src/core/render/ui.rs`: `UiCommands` into draw-list items; the UI camera; `ui_space`.
- `src/core/ui/immediate/commands.rs`: `UiCommand`, `UiCommands`.
- `src/core/shaders/core/text.wgsl`: the text and UI-rect fragment entries, composed into the sprite module.
- `benches/text.rs`.
- `examples/demos/text/`: the new demo.
- `.claude/skills/ryo-text/SKILL.md` and `src/cli/skills/ryo-text.md`.

**Modified (main ones):**
- `Cargo.toml`
- `src/lib.rs` (prelude)
- `src/core/mod.rs`
- `src/core/render/batch/{key.rs,pipelines.rs,gpu.rs}`
- `src/core/render/{draw_list.rs,frame.rs,context.rs,passes/output.rs}`
- `src/core/graphics/{sprite_gpu.rs,compositor.rs,renderer.rs,screen.rs}`
- `src/core/shaders/compose.rs`
- `src/core/engine/{mod.rs,frame.rs,extract.rs,window.rs}`
- `src/core/scene/{context.rs,manager.rs}`
- `src/core/ui/immediate/{context.rs,mod.rs}`; `src/core/ui/retained/text_input.rs`; `src/core/ui/mod.rs`
- `src/core/console/mod.rs`
- `src/core/config/debug.rs`
- `src/core/windows/{dev_window.rs,logger_window.rs,profiler_window.rs}`
- `examples/**`, `tests/**`, docs and skills

**Deleted:**
- `src/core/ui/immediate/font.rs` (`BitmapFont`, `create_debug_font`);
- the dev windows' `get_glyph`;
- `Engine::ui_font` and `Engine::ui_buffer`;
- `FrameScene::ui_buffer`;
- `Compositor::upload_ui`.

## Task Order

| # | Task | Depends on |
|---|---|---|
| 1 | Dependencies, built-in fonts, `Fonts`, baselines | none |
| 2 | Styles, rich text and layout (`TextEngine`) | 1 |
| 3 | Markup, icons, effects and reveal | 2 |
| 4 | Glyph rasterization (raster, stroke, pixel, MSDF) | 1 |
| 5 | The glyph atlas | 4 |
| 6 | Batch support: glyph and shape items, text shaders, scissor, per-camera format | 5 |
| 7 | World text (`Text2d`) | 2, 3, 6 |
| 8 | The GPU UI (commands, UI camera, `ui` pass, scale and resolution; CPU path removed) | 6, 7 |
| 9 | Widgets on the new text (`ui.text`, text input, dropdown popups) | 8 |
| 10 | Console, overlays, demo-kit chrome, dev windows | 9 |
| 11 | Examples ported and checked | 10 |
| 12 | The `text` demo, with its web build | 11 |
| 13 | Docs, skills and benchmarks | 12 |

---
### Task 1: Dependencies, built-in fonts, `Fonts`, baselines

**Files:**
- Modify: `Cargo.toml` (dependencies, the `default-fonts` feature, the `text` bench)
- Create: `src/assets/fonts/{Inter-Regular.ttf, Inter-Bold.ttf, JetBrainsMono-Regular.ttf, m5x7.ttf, OFL-Inter.txt, OFL-JetBrainsMono.txt, m5x7-LICENSE.txt, README.md}`
- Create: `src/core/text/mod.rs`, `src/core/text/fonts.rs`, `src/core/text/AGENTS.md` (first version)
- Modify: `src/core/mod.rs` (`pub mod text;`), `src/core/engine/mod.rs` (`init_world_resources` inserts `Fonts::with_builtins()`), `src/core/scene/context.rs` (`ctx.fonts()`), `src/lib.rs` (prelude: `Fonts`, `FontId`, `FontRender`, `FontRole`, `FontError`)
- Create: `benches/text.rs` (the HUD frame baseline)

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `pub struct FontId(pub(crate) u32)` (`Copy, Eq, Hash, Ord`);
  - `pub enum FontRender { Raster, Pixel { design_size: f32 }, Msdf { base: f32, range: f32 } }` (`#[non_exhaustive]`, `FontRender::pixel(size)`, `FontRender::msdf()`);
  - `pub enum FontRole { Ui, Mono, Pixel }`;
  - `pub enum FontError`;
  - `#[derive(Resource)] pub struct Fonts` with:
    - `new()`;
    - `add_bytes(impl Into<Arc<[u8]>>) -> Result<FontId, FontError>`;
    - `add_face(Arc<[u8]>, index: u32) -> Result<FontId, FontError>`;
    - `load(path) -> AssetResult<FontId>`;
    - `add_bitmap(family: &str, sprite: SpriteHandle, cell: (u32, u32), chars: &str) -> Result<FontId, FontError>`;
    - `set_render(FontId, FontRender)` and `render(FontId) -> FontRender`;
    - `family(FontId) -> Option<&str>`;
    - `find(family: &str) -> Option<FontId>`;
    - `role(FontRole) -> Option<FontId>` and `set_role(FontRole, FontId)`;
    - `prewarm(FontId, size: f32, chars: &str)`;
    - `generation() -> u64`.
  - Crate-internal:
    - `Fonts::with_builtins()`, `face(FontId) -> Option<&Face>`, `faces() -> impl Iterator<Item = (FontId, &Face)>`, `take_prewarm() -> Vec<Prewarm>`;
    - `Face { family, weight: f32, italic: bool, source: FaceSource, render: FontRender }`;
    - `FaceSource::{Outline { data: Arc<[u8]>, index: u32 }, Bitmap(BitmapFace)}`;
    - `BitmapFace { sprite, cell, columns, chars: Vec<char> }`.
  - `SceneContext::fonts(&mut self) -> Mut<'_, Fonts>`.

- [ ] **Step 1: Record the baselines before anything changes**

These numbers and images are compared in Tasks 8, 11 and 13; nothing later can recreate them once the CPU UI is gone. Do them first.

1. **The HUD bench.** Create `benches/text.rs`:

```rust
//! Text and UI frame costs: a HUD of 100 texts and 50 panels.

use criterion::{Criterion, criterion_group, criterion_main};
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// A HUD: 100 lines of text over 50 translucent panels
struct Hud;

impl Scene for Hud {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    for i in 0..50 {
      let x = (i % 10) as f32 * 64.0;
      let y = (i / 10) as f32 * 64.0;
      ui.draw_rect(Rect::new(x, y, 60.0, 60.0), Color::new(20, 20, 30, 200));
    }
    for i in 0..100 {
      let x = (i % 5) as f32 * 128.0 + 4.0;
      let y = (i / 5) as f32 * 16.0 + 4.0;
      ui.draw_text(&format!("Score {i}: {}", i * 37), x, y, Color::WHITE);
    }
    SceneAction::Continue
  }
}

fn hud_frame(c: &mut Criterion) {
  let builder = EngineBuilder::new().resolution(640, 360).start_scene(Hud);
  // Needs a GPU; without one this benchmark is skipped
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(2).expect("warm-up frames");
  c.bench_function("headless frame, HUD of 100 texts and 50 panels", |b| {
    b.iter(|| engine.step().expect("frame"))
  });
}

criterion_group!(benches, hud_frame);
criterion_main!(benches);
```

   Add the bench to `Cargo.toml` beside `foundation` and `physics`:

```toml
[[bench]]
name = "text"
harness = false
```

   Run `cargo bench --bench text -- --noplot` with nothing else CPU-heavy running. Record the mean in the task report under "Baseline (CPU UI)". Task 13 compares against it.

2. **Captures.** Capture the games and UI-heavy demos headless into `target/ui-baseline/` (git-ignored; it survives across tasks):

```bash
for game in breakout flappy bullet-hell leap-king nigh_t; do
  RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=30,120 RYO_CAPTURE_EXIT=1 \
    RYO_CAPTURE_DIR="$PWD/target/ui-baseline/$game" cargo run --release -p "$game"
done
./dev demos:tour
cp -R target/demo-tour target/ui-baseline/demo-tour
```

   If a package name differs from its folder name, check with `cargo metadata --no-deps --format-version 1 | grep '"name"'`; nigh_t's package may be named differently. Note every name you used in the report.

3. **Binary size.** `cargo build --release -p breakout`, then `ls -l target/release/breakout`. Record the byte size in the report. Task 13 measures it again.

- [ ] **Step 2: Add the dependencies and the feature**

In `Cargo.toml` `[dependencies]`, beside `etagere = "0.2"`:

```toml
# Text: layout (no system fonts), glyph rasterization, MSDF generation.
# Named only in src/core/text/.
parley = { version = "0.12", default-features = false, features = ["std"] }
swash = "0.2.10"
fdsm = "0.8"
```

Replace the `[features]` block's `default = []` with:

```toml
default = ["default-fonts"]
# Built-in fonts (Inter, JetBrains Mono, m5x7) embedded in the engine
default-fonts = []
```

Run `cargo check` (it fetches the crates). Then `cargo tree -p ryo2d -i nalgebra` and `cargo tree -d | head -40`, and note in the report any second copy of a crate the engine already has (`image`, `etagere`, `skrifa`).

- [ ] **Step 3: Add the font files**

The subset files were made by the spike: the session scratchpad's `text-spike/fonts/subset/` has `Inter-Regular.ttf`, `Inter-Bold.ttf`, `JetBrainsMono-Regular.ttf` and `m5x7.ttf`, and `text-spike/fonts/dl/` has the licence texts. Copy them into `src/assets/fonts/`, naming the licences `OFL-Inter.txt`, `OFL-JetBrainsMono.txt` and `m5x7-LICENSE.txt`. The last is a short text: "m5x7 by Daniel Linssen, CC0 1.0 Universal (public domain), https://managore.itch.io/m5x7".

If the scratchpad is gone, re-create them:
- Inter 4.1: https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip, `extras/ttf/Inter-Regular.ttf` and `Inter-Bold.ttf`.
- JetBrains Mono 2.304: https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip, `fonts/ttf/JetBrainsMono-Regular.ttf`.
- m5x7: https://commondatastorage.googleapis.com/leafo/fonts/m5x7-webfont.ttf.

Subset each with `hb-subset` (Homebrew `harfbuzz`):

```bash
U="U+0020-007E,U+00A0-024F,U+0370-03FF,U+0400-04FF,U+2010-2027,U+2030-205E,U+20AC,U+2122,U+2190-2193,U+2212,U+2500-257F,U+25A0-25FF"
hb-subset Inter-Regular.ttf --unicodes="$U" --layout-features='*' -o Inter-Regular.ttf.sub
```

(The range adds box drawing and geometric shapes, which the debug overlays and the state-machine demo draw.) If you re-subset, record the new sizes.

Write `src/assets/fonts/README.md` with, for each file: its source URL, version, licence file, original and subset size, and the exact `hb-subset` command. Expected sizes from the spike: Inter Regular 202,924 B, Bold 206,768 B, JetBrains Mono 129,632 B, m5x7 17,432 B. The m5x7 webfont is ASCII-only, so pixel text outside ASCII falls back to Inter.

Make sure `.gitattributes` treats `*.ttf` as binary (`*.ttf binary`; add the line if it is missing) so the LF rule never touches them.

- [ ] **Step 4: Write the failing tests**

Create `src/core/text/mod.rs`:

```rust
//! Text: fonts, styles, rich text and markup, layout, glyph rasterization
//! and world-space text. GPU-free: the renderer draws the glyph bitmaps
//! this module produces. `parley`, `swash` and `fdsm` are named only here.

mod fonts;

pub use fonts::{FontError, FontId, FontRender, FontRole, Fonts};
pub(crate) use fonts::{BitmapFace, Face, FaceSource, Prewarm};
```

Add `pub mod text;` to `src/core/mod.rs` beside the other modules, keeping the list's order.

Create `src/core/text/fonts.rs` with only the tests first (the module will not compile until Step 6; that is the failing run):

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_builtin_fonts_fill_every_role() {
    let fonts = Fonts::with_builtins();
    for role in [FontRole::Ui, FontRole::Mono, FontRole::Pixel] {
      assert!(fonts.role(role).is_some(), "{role:?} has a font");
    }
    let ui = fonts.role(FontRole::Ui).unwrap();
    assert_eq!(fonts.family(ui), Some("Inter"));
    let mono = fonts.role(FontRole::Mono).unwrap();
    assert_eq!(fonts.family(mono), Some("JetBrains Mono"));
    let pixel = fonts.role(FontRole::Pixel).unwrap();
    assert!(matches!(fonts.render(pixel), FontRender::Pixel { .. }));
  }

  #[test]
  fn inter_bold_is_a_heavier_face_of_the_same_family() {
    let fonts = Fonts::with_builtins();
    let mut weights: Vec<f32> = fonts
      .faces()
      .filter(|(_, face)| face.family == "Inter")
      .map(|(_, face)| face.weight)
      .collect();
    weights.sort_by(f32::total_cmp);
    assert_eq!(weights, vec![400.0, 700.0]);
  }

  #[test]
  fn find_prefers_the_regular_face() {
    let fonts = Fonts::with_builtins();
    let id = fonts.find("Inter").unwrap();
    assert_eq!(fonts.face(id).unwrap().weight, 400.0);
    assert!(fonts.find("No Such Family").is_none());
  }

  #[test]
  fn bytes_that_are_not_a_font_are_refused() {
    let mut fonts = Fonts::new();
    assert!(matches!(fonts.add_bytes(vec![1u8, 2, 3]), Err(FontError::Invalid)));
    assert!(fonts.faces().next().is_none());
  }

  #[test]
  fn every_change_bumps_the_generation() {
    let mut fonts = Fonts::with_builtins();
    let before = fonts.generation();
    let ui = fonts.role(FontRole::Ui).unwrap();
    fonts.set_render(ui, FontRender::msdf());
    assert!(fonts.generation() > before);
    assert_eq!(fonts.render(ui), FontRender::msdf());
  }

  #[test]
  fn a_bitmap_font_needs_a_cell_and_characters() {
    let mut fonts = Fonts::new();
    let sheet = crate::core::asset::SpriteHandle::from_sprite(
      crate::core::graphics::Sprite::solid_rect(64, 16, 255, 255, 255, 255),
      "sheet",
    );
    assert!(matches!(
      fonts.add_bitmap("Grid", sheet.clone(), (0, 8), "AB"),
      Err(FontError::EmptyBitmap)
    ));
    assert!(matches!(
      fonts.add_bitmap("Grid", sheet.clone(), (8, 8), ""),
      Err(FontError::EmptyBitmap)
    ));
    let id = fonts.add_bitmap("Grid", sheet, (8, 8), "ABCDEFGH").unwrap();
    assert!(matches!(fonts.render(id), FontRender::Pixel { design_size } if design_size == 8.0));
  }

  #[cfg(not(target_arch = "wasm32"))]
  #[test]
  fn loading_a_missing_file_says_not_found() {
    let mut fonts = Fonts::new();
    let err = fonts.load("does/not/exist.ttf").unwrap_err();
    assert!(matches!(err, crate::core::error::AssetError::NotFound(_)));
  }

  #[test]
  fn prewarm_requests_are_queued_for_the_engine() {
    let mut fonts = Fonts::with_builtins();
    let ui = fonts.role(FontRole::Ui).unwrap();
    fonts.prewarm(ui, 32.0, "ABC");
    let queued = fonts.take_prewarm();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].chars, "ABC");
    assert!(fonts.take_prewarm().is_empty());
  }
}
```

- [ ] **Step 5: Run the tests to see them fail**

Run: `cargo test -p ryo2d --lib text::fonts`
Expected: compile errors (`cannot find type Fonts`, `FontRole`, ...).

- [ ] **Step 6: Write `fonts.rs`**

Put this above the tests:

```rust
//! The font registry games add fonts to (`ctx.fonts()`), the built-in
//! fonts, and each font's render mode. The text engine picks up changes at
//! the start of the next frame (it compares `generation`).

use std::path::Path;
use std::sync::Arc;

use swash::{FontRef, StringId};
use thiserror::Error;

use crate::core::asset::SpriteHandle;
use crate::core::ecs::Resource;
use crate::core::error::{AssetError, AssetResult};

/// One registered font face: a font file, or one face of a collection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(pub(crate) u32);

/// How a font's glyphs are drawn
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub enum FontRender {
  /// Hinted and anti-aliased, rasterized at the exact pixel size
  #[default]
  Raster,
  /// No anti-aliasing: sizes snap to whole multiples of `design_size` and
  /// glyphs land on whole pixels
  Pixel { design_size: f32 },
  /// A multi-channel signed distance field made once at `base` pixels with
  /// a distance `range` (pixels at that size): scales freely, and the
  /// shader draws outlines, soft shadows and glow
  Msdf { base: f32, range: f32 },
}

impl FontRender {
  /// A pixel font designed at `design_size` pixels
  pub const fn pixel(design_size: f32) -> Self {
    Self::Pixel { design_size }
  }

  /// MSDF at the default base (32 px) and range (4 px)
  pub const fn msdf() -> Self {
    Self::Msdf { base: 32.0, range: 4.0 }
  }
}

/// The built-in roles a style can name instead of a family
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontRole {
  /// The UI and world font (Inter)
  #[default]
  Ui,
  /// The monospace font (JetBrains Mono): console, overlays, dev windows
  Mono,
  /// The pixel font (m5x7)
  Pixel,
}

impl FontRole {
  fn index(self) -> usize {
    match self {
      Self::Ui => 0,
      Self::Mono => 1,
      Self::Pixel => 2,
    }
  }
}

/// Why a font was refused
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FontError {
  #[error("not a TrueType or OpenType font")]
  Invalid,
  #[error("the font file has no face {0}")]
  NoFace(u32),
  #[error("a bitmap font needs a non-empty cell and at least one character")]
  EmptyBitmap,
}

/// A grid of glyphs on a sprite sheet: `chars[i]` is cell `i`, row-major
#[derive(Debug, Clone)]
pub(crate) struct BitmapFace {
  pub(crate) sprite: SpriteHandle,
  pub(crate) cell: (u32, u32),
  pub(crate) columns: u32,
  pub(crate) chars: Vec<char>,
}

/// Where a face's glyphs come from
#[derive(Debug, Clone)]
pub(crate) enum FaceSource {
  Outline { data: Arc<[u8]>, index: u32 },
  Bitmap(BitmapFace),
}

/// One registered face
#[derive(Debug, Clone)]
pub(crate) struct Face {
  pub(crate) family: String,
  pub(crate) weight: f32,
  pub(crate) italic: bool,
  pub(crate) source: FaceSource,
  pub(crate) render: FontRender,
}

/// A request to build glyphs ahead of their first use
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Prewarm {
  pub(crate) font: FontId,
  pub(crate) size: f32,
  pub(crate) chars: String,
}

/// The fonts text can use: built-in (feature `default-fonts`) and added by
/// the game. A resource: `ctx.fonts()` in scenes, `ResMut<Fonts>` in systems.
#[derive(Resource, Debug, Default)]
pub struct Fonts {
  faces: Vec<Face>,
  roles: [Option<FontId>; 3],
  generation: u64,
  prewarm: Vec<Prewarm>,
}

impl Fonts {
  /// An empty registry (no built-in fonts)
  pub fn new() -> Self {
    Self::default()
  }

  /// The registry the engine starts with: the built-in fonts when the
  /// `default-fonts` feature is on, otherwise empty
  pub(crate) fn with_builtins() -> Self {
    let mut fonts = Self::new();
    #[cfg(feature = "default-fonts")]
    builtins::register(&mut fonts);
    fonts
  }

  /// Add a TrueType or OpenType font (face 0 of a collection)
  pub fn add_bytes(&mut self, bytes: impl Into<Arc<[u8]>>) -> Result<FontId, FontError> {
    self.add_face(bytes.into(), 0)
  }

  /// Add face `index` of a font collection
  pub fn add_face(&mut self, data: Arc<[u8]>, index: u32) -> Result<FontId, FontError> {
    let font = FontRef::from_index(&data, index as usize).ok_or(if index == 0 {
      FontError::Invalid
    } else {
      FontError::NoFace(index)
    })?;
    let family = family_name(&font).ok_or(FontError::Invalid)?;
    let attributes = font.attributes();
    let face = Face {
      family,
      weight: f32::from(attributes.weight().0),
      italic: !matches!(attributes.style(), swash::Style::Normal),
      source: FaceSource::Outline { data, index },
      render: FontRender::Raster,
    };
    Ok(self.push(face))
  }

  /// Read a font file (relative paths are relative to the working
  /// directory, as sprites load by default). There is no file system on
  /// the web: embed fonts with `add_bytes(include_bytes!(..))` there.
  pub fn load(&mut self, path: impl AsRef<Path>) -> AssetResult<FontId> {
    let path = path.as_ref();
    #[cfg(target_arch = "wasm32")]
    {
      Err(AssetError::LoadError {
        path: path.to_path_buf(),
        reason: "the web has no file system: embed the font with Fonts::add_bytes".into(),
      })
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
      let bytes = std::fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
          AssetError::NotFound(path.to_path_buf())
        } else {
          AssetError::LoadError { path: path.to_path_buf(), reason: error.to_string() }
        }
      })?;
      self
        .add_bytes(bytes)
        .map_err(|error| AssetError::LoadError { path: path.to_path_buf(), reason: error.to_string() })
    }
  }

  /// A pixel font from a grid of `cell`-sized glyphs on a sprite sheet:
  /// `chars` names the cells in order, left to right, then top to bottom
  pub fn add_bitmap(
    &mut self,
    family: &str,
    sprite: SpriteHandle,
    cell: (u32, u32),
    chars: &str,
  ) -> Result<FontId, FontError> {
    if cell.0 == 0 || cell.1 == 0 || chars.is_empty() {
      return Err(FontError::EmptyBitmap);
    }
    let columns = (sprite.width() / cell.0).max(1);
    let face = Face {
      family: family.to_string(),
      weight: 400.0,
      italic: false,
      source: FaceSource::Bitmap(BitmapFace { sprite, cell, columns, chars: chars.chars().collect() }),
      render: FontRender::pixel(cell.1 as f32),
    };
    Ok(self.push(face))
  }

  /// Change how a font draws; text using it lays out again
  pub fn set_render(&mut self, font: FontId, render: FontRender) {
    if let Some(face) = self.faces.get_mut(font.0 as usize) {
      face.render = render;
      self.generation += 1;
    }
  }

  /// How a font draws (`Raster` for an unknown id)
  pub fn render(&self, font: FontId) -> FontRender {
    self.face(font).map_or(FontRender::Raster, |face| face.render)
  }

  /// The family a font belongs to
  pub fn family(&self, font: FontId) -> Option<&str> {
    self.face(font).map(|face| face.family.as_str())
  }

  /// A face of `family`, the regular (weight 400, upright) one if there
  /// is one
  pub fn find(&self, family: &str) -> Option<FontId> {
    let mut best: Option<(FontId, f32)> = None;
    for (id, face) in self.faces() {
      if face.family != family {
        continue;
      }
      let distance = (face.weight - 400.0).abs() + if face.italic { 1000.0 } else { 0.0 };
      if best.is_none_or(|(_, d)| distance < d) {
        best = Some((id, distance));
      }
    }
    best.map(|(id, _)| id)
  }

  /// The font a role uses
  pub fn role(&self, role: FontRole) -> Option<FontId> {
    self.roles[role.index()]
  }

  /// Use `font` for a role (e.g. a game's own UI font)
  pub fn set_role(&mut self, role: FontRole, font: FontId) {
    if self.face(font).is_some() {
      self.roles[role.index()] = Some(font);
      self.generation += 1;
    }
  }

  /// Build these glyphs ahead of their first use (MSDF glyphs cost about a
  /// third of a millisecond each to make): call it in a loading scene
  pub fn prewarm(&mut self, font: FontId, size: f32, chars: &str) {
    self.prewarm.push(Prewarm { font, size, chars: chars.to_string() });
  }

  /// Changes whenever a font is added or changed
  pub fn generation(&self) -> u64 {
    self.generation
  }

  pub(crate) fn face(&self, font: FontId) -> Option<&Face> {
    self.faces.get(font.0 as usize)
  }

  pub(crate) fn faces(&self) -> impl Iterator<Item = (FontId, &Face)> {
    self.faces.iter().enumerate().map(|(i, face)| (FontId(i as u32), face))
  }

  pub(crate) fn take_prewarm(&mut self) -> Vec<Prewarm> {
    std::mem::take(&mut self.prewarm)
  }

  fn push(&mut self, face: Face) -> FontId {
    let id = FontId(self.faces.len() as u32);
    self.faces.push(face);
    self.generation += 1;
    id
  }
}

/// The typographic family (name 16) if the font has one, else its family
/// (name 1)
fn family_name(font: &FontRef<'_>) -> Option<String> {
  let strings = font.localized_strings();
  strings
    .find_by_id(StringId::TypographicFamily, None)
    .or_else(|| strings.find_by_id(StringId::Family, None))
    .map(|name| name.chars().collect::<String>())
    .filter(|name| !name.is_empty())
}

#[cfg(feature = "default-fonts")]
mod builtins {
  use std::sync::Arc;

  use super::{FontRender, FontRole, Fonts};

  const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
  const INTER_BOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-Bold.ttf");
  const JETBRAINS_MONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
  const M5X7: &[u8] = include_bytes!("../../assets/fonts/m5x7.ttf");

  pub(super) fn register(fonts: &mut Fonts) {
    if let Some(ui) = add(fonts, INTER_REGULAR) {
      fonts.set_role(FontRole::Ui, ui);
    }
    let _ = add(fonts, INTER_BOLD);
    if let Some(mono) = add(fonts, JETBRAINS_MONO) {
      fonts.set_role(FontRole::Mono, mono);
    }
    if let Some(pixel) = add(fonts, M5X7) {
      fonts.set_render(pixel, FontRender::pixel(16.0));
      fonts.set_role(FontRole::Pixel, pixel);
    }
  }

  /// A built-in font that fails to parse is left out (its role stays empty
  /// and text using it falls back), never a panic
  fn add(fonts: &mut Fonts, bytes: &'static [u8]) -> Option<crate::core::text::FontId> {
    fonts.add_bytes(Arc::<[u8]>::from(bytes)).ok()
  }
}
```

If a swash 0.2.10 name fails to compile, check it against docs.rs. The spike crate (`text-spike/src/lib.rs`) compiles against the swash API. The names used here are:
- `FontRef::from_index`, `attributes().weight().0` (a `u16`), `attributes().style()` (`swash::Style::{Normal, Italic, Oblique(_)}`);
- `localized_strings().find_by_id(StringId, Option<&str>)`, `LocalizedString::chars()`.

If the m5x7 webfont's name table yields a family other than `m5x7`, that is fine: the test only checks its role and render mode.

- [ ] **Step 7: Wire the resource and the accessor**

- In `init_world_resources` (`src/core/engine/mod.rs:291`), insert `world.insert_resource(crate::core::text::Fonts::with_builtins());` beside the other resources.
- In `SceneContext` (`src/core/scene/context.rs`), next to `render_pipeline()`:

```rust
  /// The fonts text can use ([`Fonts`]): add a font file or bytes, pick a
  /// font's render mode, choose the fonts the built-in roles use
  pub fn fonts(&mut self) -> Mut<'_, Fonts> {
    self.world.resource_mut::<Fonts>()
  }
```

- In the prelude (`src/lib.rs`), add a `// Text` group: `pub use crate::core::text::{FontError, FontId, FontRender, FontRole, Fonts};`.

- [ ] **Step 8: Run the tests**

Run: `cargo test -p ryo2d --lib text::fonts`
Expected: 8 passed.

Then build without the feature: `cargo check -p ryo2d --no-default-features`. Expected: compiles (the built-in module is cfg'd out). `the_builtin_fonts_fill_every_role` and the two other tests that need built-in fonts must not run without the feature: gate them with `#[cfg(feature = "default-fonts")]`, and check with `cargo test -p ryo2d --lib --no-default-features text::fonts`.

- [ ] **Step 9: Write `src/core/text/AGENTS.md` (first version)**

```markdown
# AGENTS.md - Text

Fonts, styles, rich text and markup, layout, glyph rasterization and world
text. GPU-free: the renderer draws the bitmaps this module makes.

- `parley` (layout), `swash` (raster glyphs, font tables) and `fdsm` (MSDF)
  are named only in this module. Everything else sees `FontId`, `TextStyle`,
  `TextLayout` and `GlyphImage`.
- `Fonts` is the registry (a resource, `ctx.fonts()`). Built-in fonts come
  from `src/assets/fonts/` behind the `default-fonts` feature (on by
  default): Inter (UI and world), JetBrains Mono (console, overlays, dev
  windows), m5x7 (pixel, ASCII only). Sources, licences and the subset
  command are in `src/assets/fonts/README.md`.
- Every change to `Fonts` bumps `generation`; the text engine re-syncs and
  drops cached layouts when it changes.
- There is no file system on the web: `Fonts::load` fails there, so games
  embed fonts with `add_bytes(include_bytes!(..))`.
```

(Tasks 2-7 extend it.)

- [ ] **Step 10: Gate and commit**

Run the gate (Global Constraints), then:

```bash
git add -A
git commit -m "feat: fonts registry and built-in fonts" -m "- Inter, JetBrains Mono and m5x7 embedded behind the default-fonts feature
- ctx.fonts() adds font files or bytes and sets each font's render mode
- a HUD frame benchmark recorded before the UI moves to the GPU"
```

### Task 2: Styles, rich text and layout (`TextEngine`)

**Files:**
- Create: `src/core/text/style.rs`, `src/core/text/rich.rs`, `src/core/text/layout.rs`, `src/core/text/engine.rs`
- Modify: `src/core/text/mod.rs`, `src/core/text/AGENTS.md`, `src/lib.rs` (prelude: `TextStyle`, `TextFont`, `Outline`, `Shadow`, `Glow`, `TextLayoutOptions`, `TextAlign`, `TextOverflow`, `RichText`, `Span`, `Icon`)

**Interfaces:**
- Consumes: `Fonts`, `Face`, `FaceSource`, `FontId`, `FontRole`, `FontRender` (Task 1).
- Produces:
  - **Public:**
    - `TextStyle`: fields, `Default` (Inter via `FontRole::Ui`, 14 px, weight 400, white, line height 1.2), builders `new/font/role/family/size/weight/bold/italic/color/line_height/letter_spacing/outline/shadow/glow/fallback`;
    - `TextFont::{Role(FontRole), Family(String), Id(FontId)}`;
    - `Outline { width, color }`, `Shadow { offset: Vec2, color, softness }`, `Glow { radius, color }`;
    - `TextLayoutOptions { wrap: Option<f32>, align: TextAlign, max_lines: Option<u32>, overflow: TextOverflow, anchor: Anchor }` with builders;
    - `TextAlign::{Start, Center, End, Justify}`, `TextOverflow::{Clip, Ellipsis}`;
    - `Span` (constructors `Span::color/size/font/bold/italic/outline/effect`, builders `with_*`);
    - `Icon::{Sprite { sprite, size: Option<Vec2> }, Named { name, size: Option<Vec2> }}`;
    - `RichText::{new, plain, span, icon, icon_sized, icon_named, as_str, is_empty}` and `From<&str>`, `From<String>`.
  - **Crate-internal:**
    - `TextLayout { size: Vec2, scale: f32, glyphs: Vec<PlacedGlyph>, icons: Vec<PlacedIcon>, lines: Vec<LineInfo>, styles: Vec<GlyphStyle>, clusters: u32, pixel: bool }`;
    - `PlacedGlyph { font: FontId, glyph: u16, pos: Vec2, size: f32, style: u16, cluster: u32, skew: bool, embolden: bool, coords: u64 }`;
    - `PlacedIcon { icon: Icon, rect: Rect, cluster: u32 }`;
    - `LineInfo { top, baseline, height, range }`;
    - `GlyphStyle { color, outline, shadow, glow, effect: Option<String> }`;
    - `TextEngine::{new, sync(&Fonts), layout(&RichText, &TextStyle, &TextLayoutOptions, scale) -> Option<Arc<TextLayout>>, measure(..) -> Vec2, line_height(&TextStyle, scale) -> f32, caret_x(&str, &TextStyle, byte, scale) -> f32, byte_at(&str, &TextStyle, x, scale) -> usize, selection_x(&str, &TextStyle, Range<usize>, scale) -> (f32, f32), wrap_lines(&str, &TextStyle, width, scale) -> Vec<Range<usize>>, coords(u64) -> Option<Arc<[i16]>>, end_frame(), stats() -> CacheStats}`.

**Units.** `scale` is layout pixels per text unit: the UI passes its render-pixel scale, world text passes 1. A `TextLayout`'s positions and `size` are in layout pixels (y down, origin at the block's top-left). `measure` divides by `scale`, so callers get text units back.

- [ ] **Step 1: Write the failing tests**

Create `src/core/text/engine.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::text::{FontRole, Fonts};
  use crate::core::ui::Color;

  fn engine() -> (TextEngine, Fonts) {
    let fonts = Fonts::with_builtins();
    let mut engine = TextEngine::new();
    engine.sync(&fonts);
    (engine, fonts)
  }

  fn layout(engine: &mut TextEngine, text: impl Into<RichText>, style: &TextStyle) -> Arc<TextLayout> {
    engine
      .layout(&text.into(), style, &TextLayoutOptions::default(), 1.0)
      .expect("a layout")
  }

  #[test]
  fn plain_text_lays_out_one_glyph_per_letter() {
    let (mut engine, _) = engine();
    let laid = layout(&mut engine, "Hello", &TextStyle::default());
    assert_eq!(laid.glyphs.len(), 5);
    assert_eq!(laid.lines.len(), 1);
    assert!(laid.size.x > 20.0 && laid.size.x < 60.0, "width {}", laid.size.x);
    assert!((laid.size.y - 14.0 * 1.2).abs() < 1.5, "height {}", laid.size.y);
    let mut xs: Vec<f32> = laid.glyphs.iter().map(|g| g.pos.x).collect();
    let sorted = { let mut s = xs.clone(); s.sort_by(f32::total_cmp); s };
    assert_eq!(xs, sorted, "glyphs run left to right");
    xs.dedup();
    assert_eq!(xs.len(), 5);
  }

  #[test]
  fn the_cache_reuses_a_layout() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    let a = layout(&mut engine, "cached", &style);
    let b = layout(&mut engine, "cached", &style);
    assert!(Arc::ptr_eq(&a, &b));
    let stats = engine.stats();
    assert_eq!((stats.hits, stats.misses), (1, 1));
    let c = layout(&mut engine, "cached", &style.clone().size(20.0));
    assert!(!Arc::ptr_eq(&a, &c), "a different style is a different layout");
  }

  #[test]
  fn a_font_registration_invalidates_cached_layouts() {
    let mut fonts = Fonts::new();
    let inter = fonts
      .add_bytes(include_bytes!("../../assets/fonts/Inter-Regular.ttf").to_vec())
      .unwrap();
    fonts.set_role(FontRole::Ui, inter);
    let mut engine = TextEngine::new();
    engine.sync(&fonts);
    let style = TextStyle::default().family("JetBrains Mono");
    let before = layout(&mut engine, "abc", &style);
    assert!(before.glyphs.iter().all(|g| g.font == inter), "falls back to the UI font");

    let mono = fonts
      .add_bytes(include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf").to_vec())
      .unwrap();
    engine.sync(&fonts);
    let after = layout(&mut engine, "abc", &style);
    assert!(!Arc::ptr_eq(&before, &after));
    assert!(after.glyphs.iter().all(|g| g.font == mono), "now uses the new font");
  }

  #[test]
  fn spans_set_colour_and_size_per_run() {
    let (mut engine, _) = engine();
    let red = Color::rgb(255, 0, 0);
    let text = RichText::new("ab").span("cd", Span::color(red).with_size(28.0));
    let laid = layout(&mut engine, text, &TextStyle::default());
    assert_eq!(laid.glyphs.len(), 4);
    let styles: Vec<Color> = laid.glyphs.iter().map(|g| laid.styles[g.style as usize].color).collect();
    assert_eq!(styles, vec![Color::WHITE, Color::WHITE, red, red]);
    assert_eq!(laid.glyphs[3].size, 28.0);
    assert!(laid.size.y > 28.0, "the line grows to the big span");
  }

  #[test]
  fn wrapping_breaks_lines_and_alignment_moves_them() {
    let (mut engine, _) = engine();
    let options = TextLayoutOptions::default().wrap(60.0);
    let text: RichText = "one two three four five".into();
    let start = engine.layout(&text, &TextStyle::default(), &options, 1.0).unwrap();
    assert!(start.lines.len() >= 3);
    assert!(start.size.x <= 60.5);
    let centred = engine
      .layout(&text, &TextStyle::default(), &options.align(TextAlign::Center), 1.0)
      .unwrap();
    let first_x = |l: &TextLayout| l.glyphs.iter().map(|g| g.pos.x).fold(f32::MAX, f32::min);
    assert!(first_x(&centred) > first_x(&start));
  }

  #[test]
  fn max_lines_with_ellipsis_ends_in_an_ellipsis() {
    let (mut engine, _) = engine();
    let options = TextLayoutOptions::default()
      .wrap(60.0)
      .max_lines(1)
      .overflow(TextOverflow::Ellipsis);
    let laid = engine
      .layout(&"one two three four".into(), &TextStyle::default(), &options, 1.0)
      .unwrap();
    assert_eq!(laid.lines.len(), 1);
    assert!(laid.size.x <= 60.5);
    let ellipsis = engine.glyph_for(FontRole::Ui, '\u{2026}').unwrap();
    assert_eq!(laid.glyphs.last().map(|g| g.glyph), Some(ellipsis));
  }

  #[test]
  fn an_inline_icon_gets_a_line_high_box() {
    let (mut engine, _) = engine();
    let text = RichText::new("Press ").icon_named("a_button").plain(" to jump");
    let laid = layout(&mut engine, text, &TextStyle::default());
    assert_eq!(laid.icons.len(), 1);
    let icon = &laid.icons[0];
    assert!((icon.rect.height - 14.0).abs() < 1.0, "line-high: {}", icon.rect.height);
    assert!(icon.rect.x > 20.0, "after 'Press '");
  }

  #[test]
  fn a_character_no_font_has_lays_out_as_tofu_and_warns_once() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    let laid = layout(&mut engine, "a\u{6F22}b", &style);
    assert_eq!(laid.glyphs.len(), 3);
    assert_eq!(laid.glyphs[1].glyph, 0, "the .notdef (tofu) glyph");
    let _ = layout(&mut engine, "x\u{6F22}y", &style);
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("U+6F22") && e.message.contains("Inter"))
      .count();
    assert_eq!(warnings, 1);
  }

  #[test]
  fn degenerate_sizes_lay_out_nothing() {
    let (mut engine, _) = engine();
    for size in [0.0, -4.0, f32::NAN, f32::INFINITY] {
      let style = TextStyle::default().size(size);
      assert!(engine.layout(&"x".into(), &style, &TextLayoutOptions::default(), 1.0).is_none());
    }
    let bad_wrap = TextLayoutOptions::default().wrap(f32::NAN);
    assert!(engine.layout(&"x".into(), &TextStyle::default(), &bad_wrap, 1.0).is_none());
    assert!(engine.layout(&"x".into(), &TextStyle::default(), &TextLayoutOptions::default(), 0.0).is_none());
    assert_eq!(engine.measure(&"x".into(), &TextStyle::default().size(f32::NAN), &TextLayoutOptions::default(), 1.0), Vec2::ZERO);
  }

  #[test]
  fn carets_and_hit_tests_agree() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    let text = "héllo wörld";
    let mut last = -1.0;
    for (byte, _) in text.char_indices() {
      let x = engine.caret_x(text, &style, byte, 1.0);
      assert!(x > last, "carets move right");
      last = x;
      assert_eq!(engine.byte_at(text, &style, x + 0.1, 1.0), byte);
    }
    let (from, to) = engine.selection_x(text, &style, 0..2, 1.0);
    assert!(from < to);
  }

  #[test]
  fn wrap_lines_returns_byte_ranges() {
    let (mut engine, _) = engine();
    let text = "alpha beta gamma delta";
    let lines = engine.wrap_lines(text, &TextStyle::default(), 50.0, 1.0);
    assert!(lines.len() >= 2);
    assert_eq!(lines.first().map(|r| r.start), Some(0));
    assert_eq!(lines.last().map(|r| r.end), Some(text.len()));
  }

  #[test]
  fn scale_lays_out_in_layout_pixels_and_measure_returns_units() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    let one = engine.layout(&"Scale".into(), &style, &TextLayoutOptions::default(), 1.0).unwrap();
    let two = engine.layout(&"Scale".into(), &style, &TextLayoutOptions::default(), 2.0).unwrap();
    assert!((two.size.x - 2.0 * one.size.x).abs() < 2.0);
    assert_eq!(two.glyphs[0].size, 28.0);
    let units = engine.measure(&"Scale".into(), &style, &TextLayoutOptions::default(), 2.0);
    assert!((units.x - one.size.x).abs() < 1.0);
  }

  #[test]
  fn a_bitmap_font_lays_out_on_its_grid() {
    let mut fonts = Fonts::with_builtins();
    let sheet = crate::core::asset::SpriteHandle::from_sprite(
      crate::core::graphics::Sprite::solid_rect(24, 8, 255, 255, 255, 255),
      "grid",
    );
    fonts.add_bitmap("Grid", sheet, (8, 8), "ABC").unwrap();
    let mut engine = TextEngine::new();
    engine.sync(&fonts);
    let style = TextStyle::default().family("Grid").size(16.0);
    let laid = layout(&mut engine, "ABCA", &style);
    assert!(laid.pixel);
    let ids: Vec<u16> = laid.glyphs.iter().map(|g| g.glyph).collect();
    assert_eq!(ids, vec![0, 1, 2, 0]);
    let xs: Vec<f32> = laid.glyphs.iter().map(|g| g.pos.x).collect();
    assert_eq!(xs, vec![0.0, 16.0, 32.0, 48.0]);
  }
}
```

(The built-in-font tests need `default-fonts`; mark the module `#[cfg(all(test, feature = "default-fonts"))]`.)

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ryo2d --lib text::engine`
Expected: compile errors (`TextEngine`, `RichText`, `TextStyle` not found).

- [ ] **Step 3: Write `style.rs`**

```rust
//! Text styles and layout options

use std::hash::{Hash, Hasher};

use glam::Vec2;

use crate::core::text::{FontId, FontRole};
use crate::core::ui::{Anchor, Color};

/// Which font a style asks for
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TextFont {
  /// A built-in role's font (the game can point a role at its own font)
  Role(FontRole),
  /// A family by name
  Family(String),
  /// One exact face
  Id(FontId),
}

impl Default for TextFont {
  fn default() -> Self {
    Self::Role(FontRole::Ui)
  }
}

/// A line around each glyph
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
  /// Width in text units (pixels in the UI)
  pub width: f32,
  pub color: Color,
}

/// A copy of the text under it
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
  /// Offset in text units, +y down
  pub offset: Vec2,
  pub color: Color,
  /// Blur radius in text units (MSDF fonts only; raster shadows are hard)
  pub softness: f32,
}

/// A soft halo around the glyphs (MSDF fonts only)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glow {
  pub radius: f32,
  pub color: Color,
}

/// How text looks
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TextStyle {
  pub font: TextFont,
  /// Tried in order when the font lacks a character, before the built-in
  /// font of the font's role
  pub fallbacks: Vec<TextFont>,
  /// Size in text units: pixels in the UI, world units on entities
  pub size: f32,
  /// 100 (thin) to 900 (black); 400 is regular
  pub weight: f32,
  pub italic: bool,
  pub color: Color,
  /// Line height as a multiple of the size
  pub line_height: f32,
  /// Extra space between characters, in text units
  pub letter_spacing: f32,
  pub outline: Option<Outline>,
  pub shadow: Option<Shadow>,
  pub glow: Option<Glow>,
}

impl Default for TextStyle {
  fn default() -> Self {
    Self {
      font: TextFont::default(),
      fallbacks: Vec::new(),
      size: 14.0,
      weight: 400.0,
      italic: false,
      color: Color::WHITE,
      line_height: 1.2,
      letter_spacing: 0.0,
      outline: None,
      shadow: None,
      glow: None,
    }
  }
}

impl TextStyle {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn font(mut self, font: TextFont) -> Self {
    self.font = font;
    self
  }
  pub fn role(self, role: FontRole) -> Self {
    self.font(TextFont::Role(role))
  }
  pub fn family(self, family: impl Into<String>) -> Self {
    self.font(TextFont::Family(family.into()))
  }
  pub fn size(mut self, size: f32) -> Self {
    self.size = size;
    self
  }
  pub fn weight(mut self, weight: f32) -> Self {
    self.weight = weight;
    self
  }
  pub fn bold(self) -> Self {
    self.weight(700.0)
  }
  pub fn italic(mut self) -> Self {
    self.italic = true;
    self
  }
  pub fn color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }
  pub fn line_height(mut self, line_height: f32) -> Self {
    self.line_height = line_height;
    self
  }
  pub fn letter_spacing(mut self, spacing: f32) -> Self {
    self.letter_spacing = spacing;
    self
  }
  pub fn outline(mut self, width: f32, color: Color) -> Self {
    self.outline = Some(Outline { width, color });
    self
  }
  pub fn shadow(mut self, offset: Vec2, color: Color, softness: f32) -> Self {
    self.shadow = Some(Shadow { offset, color, softness });
    self
  }
  pub fn glow(mut self, radius: f32, color: Color) -> Self {
    self.glow = Some(Glow { radius, color });
    self
  }
  pub fn fallback(mut self, font: TextFont) -> Self {
    self.fallbacks.push(font);
    self
  }

  /// Size and line height usable for layout
  pub(crate) fn is_valid(&self) -> bool {
    self.size.is_finite()
      && self.size > 0.0
      && self.line_height.is_finite()
      && self.line_height > 0.0
      && self.weight.is_finite()
      && self.letter_spacing.is_finite()
  }

  /// Feed everything that changes layout or look into a cache key
  pub(crate) fn hash_into(&self, h: &mut impl Hasher) {
    self.font.hash(h);
    self.fallbacks.hash(h);
    for value in [self.size, self.weight, self.line_height, self.letter_spacing] {
      value.to_bits().hash(h);
    }
    self.italic.hash(h);
    self.color.hash(h);
    hash_outline(&self.outline, h);
    if let Some(shadow) = self.shadow {
      (shadow.offset.x.to_bits(), shadow.offset.y.to_bits(), shadow.softness.to_bits()).hash(h);
      shadow.color.hash(h);
    }
    if let Some(glow) = self.glow {
      glow.radius.to_bits().hash(h);
      glow.color.hash(h);
    }
  }
}

pub(crate) fn hash_outline(outline: &Option<Outline>, h: &mut impl Hasher) {
  if let Some(outline) = outline {
    outline.width.to_bits().hash(h);
    outline.color.hash(h);
  }
}

/// Horizontal alignment of each line within the block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
  #[default]
  Start,
  Center,
  End,
  /// Stretch spaces so every line but the last fills the wrap width
  Justify,
}

/// What happens to text past `max_lines`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextOverflow {
  #[default]
  Clip,
  /// The last kept line ends in "…"
  Ellipsis,
}

/// How a text block is broken and placed
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TextLayoutOptions {
  /// Wrap width in text units; `None` keeps each line whole
  pub wrap: Option<f32>,
  pub align: TextAlign,
  pub max_lines: Option<u32>,
  pub overflow: TextOverflow,
  /// The point of the block that sits on the text's position
  pub anchor: Anchor,
}

impl Default for TextLayoutOptions {
  fn default() -> Self {
    Self { wrap: None, align: TextAlign::Start, max_lines: None, overflow: TextOverflow::Clip, anchor: Anchor::TopLeft }
  }
}

impl TextLayoutOptions {
  pub fn wrap(mut self, width: f32) -> Self {
    self.wrap = Some(width);
    self
  }
  pub fn align(mut self, align: TextAlign) -> Self {
    self.align = align;
    self
  }
  pub fn max_lines(mut self, lines: u32) -> Self {
    self.max_lines = Some(lines);
    self
  }
  pub fn overflow(mut self, overflow: TextOverflow) -> Self {
    self.overflow = overflow;
    self
  }
  pub fn anchor(mut self, anchor: Anchor) -> Self {
    self.anchor = anchor;
    self
  }

  pub(crate) fn is_valid(&self) -> bool {
    self.wrap.is_none_or(|w| w.is_finite() && w >= 0.0)
  }

  pub(crate) fn hash_into(&self, h: &mut impl Hasher) {
    self.wrap.map(f32::to_bits).hash(h);
    self.align.hash(h);
    self.max_lines.hash(h);
    self.overflow.hash(h);
    // The anchor only places the block; it never changes the layout
  }
}
```

- [ ] **Step 4: Write `rich.rs`**

```rust
//! Rich text: one string with styled spans, inline icons, typewriter pauses
//! and effect ranges. Built by appending, so spans never overlap.

use std::hash::{Hash, Hasher};
use std::ops::Range;

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::text::style::{Outline, TextFont, hash_outline};
use crate::core::ui::Color;

/// Overrides for one run of text; unset fields keep the base style
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Span {
  pub color: Option<Color>,
  pub size: Option<f32>,
  pub font: Option<TextFont>,
  pub weight: Option<f32>,
  pub italic: Option<bool>,
  pub outline: Option<Outline>,
  /// A per-glyph effect by name (`wave`, `shake`, `rainbow`, or one the
  /// game registered in `TextEffects`)
  pub effect: Option<String>,
}

impl Span {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn color(color: Color) -> Self {
    Self::new().with_color(color)
  }
  pub fn size(size: f32) -> Self {
    Self::new().with_size(size)
  }
  pub fn font(font: TextFont) -> Self {
    Self::new().with_font(font)
  }
  pub fn bold() -> Self {
    Self::new().with_weight(700.0)
  }
  pub fn italic() -> Self {
    Self::new().with_italic(true)
  }
  pub fn outline(width: f32, color: Color) -> Self {
    Self::new().with_outline(width, color)
  }
  pub fn effect(name: impl Into<String>) -> Self {
    Self::new().with_effect(name)
  }
  pub fn with_color(mut self, color: Color) -> Self {
    self.color = Some(color);
    self
  }
  pub fn with_size(mut self, size: f32) -> Self {
    self.size = Some(size);
    self
  }
  pub fn with_font(mut self, font: TextFont) -> Self {
    self.font = Some(font);
    self
  }
  pub fn with_weight(mut self, weight: f32) -> Self {
    self.weight = Some(weight);
    self
  }
  pub fn with_italic(mut self, italic: bool) -> Self {
    self.italic = Some(italic);
    self
  }
  pub fn with_outline(mut self, width: f32, color: Color) -> Self {
    self.outline = Some(Outline { width, color });
    self
  }
  pub fn with_effect(mut self, name: impl Into<String>) -> Self {
    self.effect = Some(name.into());
    self
  }

  /// This span laid over another (the markup parser nests tags with it)
  pub(crate) fn over(&self, base: &Span) -> Span {
    Span {
      color: self.color.or(base.color),
      size: self.size.or(base.size),
      font: self.font.clone().or_else(|| base.font.clone()),
      weight: self.weight.or(base.weight),
      italic: self.italic.or(base.italic),
      outline: self.outline.or(base.outline),
      effect: self.effect.clone().or_else(|| base.effect.clone()),
    }
  }

  fn hash_into(&self, h: &mut impl Hasher) {
    self.color.hash(h);
    self.size.map(f32::to_bits).hash(h);
    self.font.hash(h);
    self.weight.map(f32::to_bits).hash(h);
    self.italic.hash(h);
    hash_outline(&self.outline, h);
    self.effect.hash(h);
  }
}

/// A picture inline with text
#[derive(Debug, Clone, PartialEq)]
pub enum Icon {
  /// A sprite; `size` in text units, default a square the line's height
  Sprite { sprite: SpriteHandle, size: Option<Vec2> },
  /// A name looked up in `TextIcons` when the text is drawn
  Named { name: String, size: Option<Vec2> },
}

impl Icon {
  pub(crate) fn size(&self) -> Option<Vec2> {
    match self {
      Self::Sprite { size, .. } | Self::Named { size, .. } => *size,
    }
  }

  fn hash_into(&self, h: &mut impl Hasher) {
    match self {
      Self::Sprite { sprite, size } => {
        0u8.hash(h);
        sprite.serial().hash(h);
        size.map(|s| (s.x.to_bits(), s.y.to_bits())).hash(h);
      }
      Self::Named { name, size } => {
        1u8.hash(h);
        name.hash(h);
        size.map(|s| (s.x.to_bits(), s.y.to_bits())).hash(h);
      }
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SpanMark {
  pub(crate) range: Range<usize>,
  pub(crate) span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IconMark {
  /// Byte index the icon sits before
  pub(crate) at: usize,
  pub(crate) icon: Icon,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PauseMark {
  pub(crate) at: usize,
  pub(crate) seconds: f32,
}

/// Text with styled spans, inline icons and typewriter pauses
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RichText {
  pub(crate) text: String,
  pub(crate) spans: Vec<SpanMark>,
  pub(crate) icons: Vec<IconMark>,
  pub(crate) pauses: Vec<PauseMark>,
}

impl RichText {
  /// Plain text in the base style
  pub fn new(text: impl Into<String>) -> Self {
    Self { text: text.into(), ..Self::default() }
  }

  /// Append text in the base style
  pub fn plain(mut self, text: &str) -> Self {
    self.text.push_str(text);
    self
  }

  /// Append text with overrides
  pub fn span(mut self, text: &str, span: Span) -> Self {
    let start = self.text.len();
    self.text.push_str(text);
    if !text.is_empty() {
      self.spans.push(SpanMark { range: start..self.text.len(), span });
    }
    self
  }

  /// Append a sprite as big as the line is high
  pub fn icon(self, sprite: SpriteHandle) -> Self {
    self.push_icon(Icon::Sprite { sprite, size: None })
  }

  /// Append a sprite at a size in text units
  pub fn icon_sized(self, sprite: SpriteHandle, size: Vec2) -> Self {
    self.push_icon(Icon::Sprite { sprite, size: Some(size) })
  }

  /// Append the icon `TextIcons` maps this name to
  pub fn icon_named(self, name: impl Into<String>) -> Self {
    self.push_icon(Icon::Named { name: name.into(), size: None })
  }

  /// The characters (icons and markup excluded)
  pub fn as_str(&self) -> &str {
    &self.text
  }

  pub fn is_empty(&self) -> bool {
    self.text.is_empty() && self.icons.is_empty()
  }

  fn push_icon(mut self, icon: Icon) -> Self {
    self.icons.push(IconMark { at: self.text.len(), icon });
    self
  }

  pub(crate) fn hash_into(&self, h: &mut impl Hasher) {
    self.text.hash(h);
    for mark in &self.spans {
      (mark.range.start, mark.range.end).hash(h);
      mark.span.hash_into(h);
    }
    for mark in &self.icons {
      mark.at.hash(h);
      mark.icon.hash_into(h);
    }
    // Pauses only time the reveal; they never change the layout
  }
}

impl From<&str> for RichText {
  fn from(text: &str) -> Self {
    Self::new(text)
  }
}

impl From<String> for RichText {
  fn from(text: String) -> Self {
    Self::new(text)
  }
}
```

`SpriteHandle::serial()` is `pub(crate)` (`asset/manager.rs:89`), which this crate-internal hash can use.

- [ ] **Step 5: Write `layout.rs` (the output types)**

```rust
//! A laid-out text block: positioned glyphs and icon boxes in layout pixels
//! (y down, origin at the block's top-left)

use std::ops::Range;

use glam::Vec2;

use crate::core::text::FontId;
use crate::core::text::rich::Icon;
use crate::core::text::style::{Glow, Outline, Shadow};
use crate::core::ui::{Color, Rect};

/// The look of one run, resolved from the base style and its span
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlyphStyle {
  pub(crate) color: Color,
  pub(crate) outline: Option<Outline>,
  pub(crate) shadow: Option<Shadow>,
  pub(crate) glow: Option<Glow>,
  pub(crate) effect: Option<String>,
}

/// One glyph, placed
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlacedGlyph {
  pub(crate) font: FontId,
  /// Glyph id in its font (for a bitmap font: the cell index)
  pub(crate) glyph: u16,
  /// The glyph's origin on its baseline, layout pixels
  pub(crate) pos: Vec2,
  /// Font size, layout pixels
  pub(crate) size: f32,
  /// Index into `TextLayout::styles`
  pub(crate) style: u16,
  /// Its character cluster in logical order (reveal and effects count these)
  pub(crate) cluster: u32,
  /// Synthesized italic (a skew) and bold (an embolden)
  pub(crate) skew: bool,
  pub(crate) embolden: bool,
  /// Hash of a variable font's normalized coordinates; 0 for none
  pub(crate) coords: u64,
}

/// An inline icon's box
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlacedIcon {
  pub(crate) icon: Icon,
  pub(crate) rect: Rect,
  /// Revealed together with this cluster
  pub(crate) cluster: u32,
}

/// One line of the block
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LineInfo {
  pub(crate) top: f32,
  pub(crate) baseline: f32,
  pub(crate) height: f32,
  pub(crate) range: Range<usize>,
}

/// A text block ready to draw
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct TextLayout {
  /// Layout pixels
  pub(crate) size: Vec2,
  /// Layout pixels per text unit
  pub(crate) scale: f32,
  pub(crate) glyphs: Vec<PlacedGlyph>,
  pub(crate) icons: Vec<PlacedIcon>,
  pub(crate) lines: Vec<LineInfo>,
  pub(crate) styles: Vec<GlyphStyle>,
  /// Character clusters in the block (the reveal's unit)
  pub(crate) clusters: u32,
  /// Every glyph is a pixel or bitmap font's
  pub(crate) pixel: bool,
}
```

- [ ] **Step 6: Write `engine.rs` (the `TextEngine`)**

The Parley calls below are the spike's working calls (`text-spike/REPORT.md` §2, probes `p1_layout`, `p1b_variable_tree`). Where a name differs in parley 0.12.0, follow the spike's code. Two names were not exercised by the spike; check them on docs.rs and adapt:
- `StyleProperty::FontStack(FontStack::List(..))`, for the fallback list;
- `FontInfoOverride { family_name, .. }`.

```rust
//! The engine's text engine: Parley contexts over the registered fonts, the
//! layout cache, measuring and hit-testing. Engine-owned (not a resource),
//! so the UI pass and the world-text extract can borrow it mutably while
//! scenes hold the world.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use glam::Vec2;
use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{
  Affinity, Alignment, AlignmentOptions, Cursor, FontContext, FontFamily, FontStack, FontStyle,
  FontWeight, InlineBox, InlineBoxKind, Layout, LayoutContext, LineHeight, PositionedLayoutItem,
  Selection, StyleProperty, VerticalAlign,
};

use crate::core::text::layout::{GlyphStyle, LineInfo, PlacedGlyph, PlacedIcon, TextLayout};
use crate::core::text::rich::RichText;
use crate::core::text::style::{TextAlign, TextFont, TextLayoutOptions, TextOverflow, TextStyle};
use crate::core::text::{BitmapFace, FaceSource, FontId, FontRender, FontRole, Fonts};
use crate::core::ui::Rect;

/// Cached layouts unused for this many frames are dropped
const CACHE_FRAMES: u64 = 120;

/// Cache counters (tests and the benchmark read them)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CacheStats {
  pub(crate) hits: u64,
  pub(crate) misses: u64,
}

struct Cached {
  layout: Arc<TextLayout>,
  last_used: u64,
}

/// What the engine knows about one registered face
struct FaceInfo {
  family: String,
  weight: f32,
  italic: bool,
  render: FontRender,
  bitmap: Option<BitmapFace>,
  /// Outline faces: the font data, for charmap checks
  data: Option<(Arc<[u8]>, u32)>,
}

pub(crate) struct TextEngine {
  generation: Option<u64>,
  fcx: FontContext,
  lcx: LayoutContext<u16>,
  faces: HashMap<FontId, FaceInfo>,
  /// Parley font (blob id, face index) to the registry's id
  by_blob: HashMap<(u64, u32), FontId>,
  roles: [Option<FontId>; 3],
  cache: HashMap<u64, Cached>,
  /// Single-line plain layouts for carets and hit tests
  lines: HashMap<u64, (Layout<u16>, u64)>,
  coords: HashMap<u64, Arc<[i16]>>,
  warned: HashSet<u64>,
  frame: u64,
  stats: CacheStats,
}
```

Then implement these methods. Each paragraph says what the method must do; write each as straightforward code over the Parley calls shown.

- **`new()`**
  - An empty engine: `FontContext { collection: Collection::new(CollectionOptions { shared: false, system_fonts: false }), source_cache: SourceCache::default() }`, `LayoutContext::new()`.
  - `generation: None`.
- **`sync(&mut self, fonts: &Fonts)`**
  - Return at once when `Some(fonts.generation()) == self.generation`.
  - Otherwise rebuild everything: a fresh `FontContext` (as in `new`), `faces`, `by_blob` and `roles` (from `fonts.role(..)` for each role).
  - Clear `cache` and `lines`.
  - For each `(id, face)` in `fonts.faces()`:
    - **Outline face:** make `Blob::new(Arc::new(data.clone()))`. Register it with `fcx.collection.register_fonts(blob, Some(FontInfoOverride { family_name: Some(&face.family), ..Default::default() }))`, so Parley's family names are the registry's. Record `by_blob.insert((blob_id, index), id)`, taking the blob's id (`Blob::id()`) before moving it in.
    - **Bitmap face:** keep its `BitmapFace` in `FaceInfo.bitmap` and register nothing with Parley.
  - Set `generation = Some(fonts.generation())`.
- **`resolve(&self, font: &TextFont) -> Option<FontId>`**
  - `Role(r)` is `roles[r]`.
  - `Id(id)` is the id if `faces` has it.
  - `Family(name)` is the face of that family closest to weight 400 and upright (as `Fonts::find`).
- **`stack(&self, style: &TextStyle, span_font: Option<&TextFont>) -> Vec<String>`**
  - The family names Parley should try, in order:
    1. the span's font, else the style's;
    2. the style's fallbacks;
    3. the family of the `FontRole::Ui` font;
    4. the family of the `FontRole::Mono` font. JetBrains Mono covers the box-drawing characters, arrows and shapes Inter lacks, which the overlays and some demos draw.
  - Leave out names nothing resolves and duplicates.
  - For an unknown family, warn once (key: the name's hash): `engine_warn!(module: "text", "no font family named {name:?}; using the fallbacks")`.
- **`layout(&mut self, text, style, options, scale) -> Option<Arc<TextLayout>>`**
  1. Return `None` if `!style.is_valid() || !options.is_valid() || !scale.is_finite() || scale <= 0.0`. Warn once per style hash: `engine_warn!(module: "text", "text style with size {} cannot be laid out; nothing is drawn", style.size)`.
  2. Hash `(text, style, options, scale.to_bits(), self.generation)` with `std::hash::DefaultHasher`. On a hit, set `last_used = frame`, count a hit and return the `Arc`.
  3. On a miss, count it. If the resolved base font is a bitmap face, call `layout_bitmap(..)`; otherwise call `layout_parley(..)`. Wrap the result in an `Arc`, insert it, and return it.
- **`layout_parley(&mut self, text, style, options, scale) -> TextLayout`**
  1. **Builder:** `let mut builder = self.lcx.ranged_builder(&mut self.fcx, &text.text, scale, true);`.
  2. **Defaults:**
     - `FontStack(FontStack::List(Cow::Owned(stack.iter().map(|n| FontFamily::named(n)).collect())))` (check the name, as noted above);
     - `FontSize(style.size)`;
     - `FontWeight(FontWeight::new(style.weight))`;
     - `FontStyle(if style.italic { FontStyle::Italic } else { FontStyle::Normal })`;
     - `Brush(0u16)`;
     - `LineHeight::FontSizeRelative(style.line_height)`;
     - `LetterSpacing(style.letter_spacing)`.
  3. **Spans:** for span `i` (`text.spans[i]`), push `Brush((i + 1) as u16)` over its range, then whichever overrides the span sets (size, the stack for its font, weight, italic).
  4. **Styles:** `styles[0]` is the base, and `styles[i + 1]` the base with span `i`'s colour, outline and effect.
  5. **Icons:** push each as `InlineBox { id: index as u64, kind: InlineBoxKind::InFlow, index: mark.at, width, height, baseline: None, vertical_align: VerticalAlign::BASELINE }`. The width and height are `icon.size().unwrap_or(Vec2::splat(style.size)) * scale`.
  6. **Build and break:** `let mut parley_layout = builder.build(&text.text);`, then `break_all_lines(options.wrap.map(|w| w * scale))`, then `align(alignment, AlignmentOptions::default())`, mapping `Start/Center/End/Justify` to Parley's `Alignment::Start/Center/End/Justify`.
  7. **Walk** `lines()` and `items()` as in the spike:
     - **`GlyphRun`:** for each positioned glyph push a `PlacedGlyph`:
       - `font = by_blob[(run.font().data.id(), run.font().index)]`; a run of an unknown font is skipped and warned once;
       - `glyph = g.id as u16`, `pos = (g.x, g.y)`, `size = run.font_size()`, `style = brush`;
       - `skew = run.synthesis().skew().is_some()`, `embolden = run.synthesis().embolden()`;
       - `coords`: the hash of `run.normalized_coords()` as `i16` bits, stored in `self.coords` once (0 when empty).
     - **`InlineBox`:** push a `PlacedIcon` with the mark's icon and `Rect::new(b.x, b.y, b.width, b.height)`.
     - **`LineInfo`:** from `line.metrics()`: `top = block_min_coord`, `baseline`, `height = line_height`, `range = line.text_range()`.
  8. **Clusters** (logical order):
     1. Collect the byte start of every cluster: walk `run.visual_clusters()` for each run. Each `Cluster`'s `glyphs()` come in the same order as `positioned_glyphs()`, so record per glyph its cluster's `text_range().start`.
     2. Sort and dedup all cluster starts into a vector.
     3. Each glyph's `cluster` is its start's index in that vector. Each icon's `cluster` is the index of the first cluster at or after its byte.
     4. `TextLayout::clusters` is the vector's length.

     If `visual_clusters` is named differently in 0.12, use whichever cluster iterator yields `Cluster`s with `glyphs()` and `text_range()` (the spike's `Cluster::from_byte_index` walk is a fallback: walk `next_logical()` from byte 0).
  9. **Max lines:** when `options.max_lines` is `Some(n)` and the layout has more lines, keep the glyphs, icons and `LineInfo`s of the first `n` lines only.

     With `TextOverflow::Ellipsis`:
     1. Lay out "…" alone (base style, same scale) with `layout_parley` on a plain `RichText`.
     2. Drop glyphs from the end of line `n - 1` while `last.pos.x + advance > wrap_px - ellipsis_width`. A glyph's advance is the next glyph's `pos.x` minus its own, or the line's `advance` for the last.
     3. Append the ellipsis glyph after the last kept glyph's end.
  10. **Size:** `(parley_layout.width(), sum of kept line heights)`. With a wrap width, clamp the x to `wrap * scale`.
  11. **Missing glyphs:** for every glyph with id 0, the character at its cluster start (`text.text[start..].chars().next()`), unless it is whitespace or a control character, is warned once per (family of the first stack name, character): `engine_warn!(module: "text", "no font has {:?} (U+{:04X}); drawn as {}'s box", c, c as u32, family)`. The tofu box is glyph 0 itself.
  12. **Pixel:** `pixel = every glyph's face render is FontRender::Pixel`.
- **`layout_bitmap(&mut self, text, style, options, scale) -> TextLayout`**
  - A monospace grid layout. With `cell = (cw, ch)` and `k = (style.size * scale) / ch as f32`, each `char` advances `cw * k`. A character the face lacks uses `'?'` if the face has it; otherwise it is skipped and warned once per (face, character).
  - Lines break at `'\n'`, and at the last space before the wrap width when one is set.
  - Line height is `ch * k * style.line_height`, baselines are at `top + ch * k`, and the alignment offset follows the line's width within the block.
  - Icons take `icon.size().unwrap_or(Vec2::splat(style.size)) * scale` and advance the pen by their width.
  - `PlacedGlyph.glyph` is the cell index (`chars.iter().position`), `size = ch * k`, and `cluster` counts characters.
  - A span setting an outline font (or the reverse) is warned once and ignored; colour and effect spans apply as in Parley.
  - `pixel = true`.
- **`measure(text, style, options, scale) -> Vec2`**: `layout(..).map_or(Vec2::ZERO, |l| l.size / scale)`.
- **`line_height(style, scale) -> f32`**: `style.size * style.line_height` for an outline font, `ch * k * style.line_height / scale` for a bitmap font (in text units).
- **`single_line(&mut self, text: &str, style, scale) -> Option<&Layout<u16>>`**
  - A plain one-line Parley layout cached in `lines` by `(text, style, scale, generation)`, touched each use.
  - **`caret_x(text, style, byte, scale)`**: `Cursor::from_byte_index(layout, byte, Affinity::Downstream).geometry(layout, 1.0).x0 as f32 / scale`.
  - **`byte_at(text, style, x, scale)`**: `Cursor::from_point(layout, x * scale, 0.0).index()`.
  - **`selection_x(text, style, range, scale)`**: `Selection::new(from, to).geometry(layout)`, giving the first rectangle's `x0`, `x1` divided by `scale`. Use `(0.0, 0.0)` for an empty or invalid range.
  - Bitmap faces compute the same three with their grid advances.
  - All return 0 for an invalid style.
- **`wrap_lines(text, style, width, scale) -> Vec<Range<usize>>`**: lay out plain `text` with `wrap(width)` and return each line's `text_range()`. A single `0..text.len()` for an invalid width.
- **`glyph_for(&mut self, role: FontRole, c: char) -> Option<u16>`** (for tests and the ellipsis): lay out `c` with the role's font and return the first glyph's id.
- **`coords(&self, hash: u64) -> Option<Arc<[i16]>>`**: the coordinates a glyph's `coords` hash names.
- **`face_render(&self, font: FontId) -> FontRender`** and **`face_family(&self, font: FontId) -> Option<&str>`**: for the emitter in Tasks 7 and 8.
- **`end_frame(&mut self)`**: `frame += 1`; every 60 frames, drop `cache` and `lines` entries with `frame - last_used > CACHE_FRAMES`.
- **`stats(&self) -> CacheStats`**.

Warn-once keys are hashes of the warning's subject, stored in `warned`. Never warn from a cache hit.

Register the modules in `mod.rs`:

```rust
mod engine;
mod layout;
mod rich;
mod style;

pub(crate) use engine::{CacheStats, TextEngine};
pub(crate) use layout::{GlyphStyle, LineInfo, PlacedGlyph, PlacedIcon, TextLayout};
pub use rich::{Icon, RichText, Span};
pub use style::{Glow, Outline, Shadow, TextAlign, TextFont, TextLayoutOptions, TextOverflow, TextStyle};
```

Add the public types to the prelude's `// Text` group.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p ryo2d --lib text::`
Expected: every `text::` test passes (Task 1's 8 and this task's 13).

If `the_cache_reuses_a_layout` reports two misses, the hash includes something unstable: a pointer, or `generation` before the first `sync`.

- [ ] **Step 8: Extend `src/core/text/AGENTS.md`**

Append:

```markdown
## Layout

- `TextEngine` (engine-owned, crate-internal) holds one Parley
  `FontContext` (no system fonts) and `LayoutContext`, rebuilt when
  `Fonts::generation` changes. Layouts are cached by text, style, options,
  scale and font generation, and dropped after 120 unused frames, so an
  immediate-mode call every frame costs a hash lookup.
- `scale` is layout pixels per text unit (the UI's render scale; 1 for
  world text). Layouts are in layout pixels, y down; `measure` returns
  text units.
- Styled spans never overlap (`RichText` appends); Parley's brush is the
  span index, and `TextLayout::styles` resolves each to colour, outline,
  shadow, glow and effect.
- Fallback: a style's font, its fallbacks, then the UI font, then the mono font. A character no
  font has draws glyph 0 (the font's box) and is warned once per family and
  character.
- Bitmap fonts (`Fonts::add_bitmap`) are laid out by a monospace grid
  layouter; a text uses bitmap or outline fonts, not both.
- Clusters: each glyph and icon carries its character cluster in logical
  order; reveal and per-glyph effects count clusters.
```

- [ ] **Step 9: Gate and commit**

```bash
git add -A
git commit -m "feat: text styles, rich text and cached layout" -m "- Parley lays out styled spans, inline icon boxes, wrapping, alignment and ellipsis
- layouts are cached, so immediate-mode text costs a lookup per frame
- carets and hit tests for text editing; bitmap fonts on a monospace grid"
```

### Task 3: Markup, icons, effects and reveal

**Files:**
- Create: `src/core/text/markup.rs`, `src/core/text/icons.rs`, `src/core/text/effects.rs`
- Modify: `src/core/text/rich.rs` (`pause`), `src/core/text/layout.rs` (`pauses`, `punctuation`), `src/core/text/engine.rs` (fill them), `src/core/text/mod.rs`, `src/core/engine/mod.rs` (insert `TextIcons` and `TextEffects` in `init_world_resources`), `src/lib.rs` (prelude: `TextIcons`, `TextEffects`, `GlyphContext`, `GlyphMod`), `src/core/text/AGENTS.md`

**Interfaces:**
- Consumes: `RichText`, `Span`, `Icon`, `TextLayout`, `TextEngine` (Task 2).
- Produces:
  - **Public:**
    - `RichText::markup(&str) -> RichText` and `RichText::pause(self, seconds) -> Self`;
    - `#[derive(Resource)] TextIcons` with `insert(name, SpriteHandle)`, `insert_region(name, SpriteHandle, Rect)` (pixel rect on the sprite), `get(name) -> Option<&IconSource>`, `remove(name)`;
    - `#[derive(Resource)] TextEffects` (built-ins registered by `Default`) with `register(name, impl Fn(&GlyphContext) -> GlyphMod + Send + Sync + 'static)`, `contains(name)`;
    - `GlyphContext { index: u32, cluster: u32, line: u32, time: f32, size: f32, seed: u64, args: String }` (`#[non_exhaustive]`; the effect reads fields);
    - `GlyphMod { offset: Vec2, scale: f32, color: Option<Color>, alpha: f32 }` (`Default`: zero offset, scale 1, no colour, alpha 1).
  - **Crate-internal:**
    - `IconSource { sprite, region: Option<Rect> }`;
    - `TextEffects::apply(&self, effect: &str, ctx: &GlyphContext) -> Option<GlyphMod>` (`None` for an unknown name, after warning once);
    - `reveal_alpha(cluster: u32, shown: f32, fade_clusters: f32) -> f32`;
    - `RevealClock { shown: f32, hold: f32, done: bool }` with `step(&mut self, dt, per_second, punctuation_pause, layout: &TextLayout) -> bool` (true on the step it finishes);
    - `TextLayout::{pauses: Vec<(u32, f32)>, punctuation: Vec<u32>}`.

- [ ] **Step 1: Write the failing tests**

In `markup.rs`, tests first:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::text::Span;
  use crate::core::ui::Color;

  #[test]
  fn plain_text_passes_through() {
    let rich = RichText::markup("hello world");
    assert_eq!(rich.as_str(), "hello world");
    assert!(rich.spans.is_empty());
  }

  #[test]
  fn a_colour_tag_makes_a_span() {
    let rich = RichText::markup("Deal [color=#ff4444]20[/color] damage");
    assert_eq!(rich.as_str(), "Deal 20 damage");
    assert_eq!(rich.spans.len(), 1);
    assert_eq!(rich.spans[0].range, 5..7);
    assert_eq!(rich.spans[0].span.color, Some(Color::rgb(255, 68, 68)));
  }

  #[test]
  fn nested_tags_merge_into_flat_spans() {
    let rich = RichText::markup("[b]bold [color=red]both[/color][/b] plain");
    assert_eq!(rich.as_str(), "bold both plain");
    let spans: Vec<(std::ops::Range<usize>, Span)> =
      rich.spans.iter().map(|m| (m.range.clone(), m.span.clone())).collect();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].0, 0..5);
    assert_eq!(spans[0].1.weight, Some(700.0));
    assert_eq!(spans[1].0, 5..9);
    assert_eq!(spans[1].1.weight, Some(700.0));
    assert_eq!(spans[1].1.color, Some(Color::rgb(255, 0, 0)));
  }

  #[test]
  fn icons_pauses_and_escapes() {
    let rich = RichText::markup("Press [icon=a_button] then[pause=0.5] wait [[not a tag]");
    assert_eq!(rich.as_str(), "Press  then wait [not a tag]");
    assert_eq!(rich.icons.len(), 1);
    assert_eq!(rich.icons[0].at, 6);
    assert_eq!(rich.pauses.len(), 1);
    assert_eq!(rich.pauses[0].seconds, 0.5);
  }

  #[test]
  fn unknown_tags_are_effects() {
    let rich = RichText::markup("[wave]hi[/wave] [sparkle=3]yo[/sparkle]");
    assert_eq!(rich.as_str(), "hi yo");
    assert_eq!(rich.spans[0].span.effect.as_deref(), Some("wave"));
    assert_eq!(rich.spans[1].span.effect.as_deref(), Some("sparkle=3"));
  }

  #[test]
  fn malformed_markup_renders_literally_and_never_panics() {
    for source in [
      "[color=red]never closed",
      "closed [/b] without open",
      "[size=huge]bad value[/size]",
      "dangling [ bracket",
      "[]",
      "[=x]y",
      "[color=]x[/color]",
      "\u{1F600}[b]\u{1F600}[/b]",
    ] {
      let rich = RichText::markup(source);
      assert!(!rich.as_str().is_empty() || source.is_empty(), "{source}");
    }
    let rich = RichText::markup("[color=red]never closed");
    assert_eq!(rich.as_str(), "[color=red]never closed");
    let rich = RichText::markup("[size=huge]bad value[/size]");
    assert_eq!(rich.as_str(), "[size=huge]bad value[/size]");
  }

  #[test]
  fn each_bad_markup_string_warns_once() {
    let source = "[color=red]warn me once";
    let _ = RichText::markup(source);
    let _ = RichText::markup(source);
    let count = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("warn me once"))
      .count();
    assert_eq!(count, 1);
  }
}
```

In `effects.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  fn ctx(cluster: u32, time: f32) -> GlyphContext {
    GlyphContext { index: cluster, cluster, line: 0, time, size: 20.0, seed: 7, args: String::new() }
  }

  #[test]
  fn wave_moves_glyphs_up_and_down_over_time() {
    let effects = TextEffects::default();
    let a = effects.apply("wave", &ctx(0, 0.0)).unwrap();
    let b = effects.apply("wave", &ctx(0, 0.125)).unwrap();
    assert_ne!(a.offset.y, b.offset.y);
    assert!(a.offset.y.abs() <= 20.0 * 0.15 + 1e-4);
  }

  #[test]
  fn wave_reads_its_arguments() {
    let effects = TextEffects::default();
    let mut c = ctx(0, 0.125);
    c.args = "5,1".into();
    let m = effects.apply("wave", &c).unwrap();
    assert!(m.offset.y.abs() <= 5.0 + 1e-4);
  }

  #[test]
  fn shake_is_bounded_and_rainbow_colours() {
    let effects = TextEffects::default();
    for cluster in 0..20 {
      let m = effects.apply("shake", &ctx(cluster, cluster as f32 * 0.1)).unwrap();
      assert!(m.offset.length() <= 20.0 * 0.06 * 1.5);
    }
    let c = effects.apply("rainbow", &ctx(3, 0.0)).unwrap();
    assert!(c.color.is_some());
  }

  #[test]
  fn a_registered_effect_runs_and_an_unknown_one_warns_once() {
    let mut effects = TextEffects::default();
    effects.register("lift", |ctx: &GlyphContext| GlyphMod {
      offset: glam::Vec2::new(0.0, -(ctx.cluster as f32)),
      ..GlyphMod::default()
    });
    assert_eq!(effects.apply("lift", &ctx(4, 0.0)).unwrap().offset.y, -4.0);
    assert!(effects.apply("nope-effect", &ctx(0, 0.0)).is_none());
    assert!(effects.apply("nope-effect", &ctx(0, 0.0)).is_none());
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("nope-effect"))
      .count();
    assert_eq!(warned, 1);
  }

  #[test]
  fn reveal_alpha_steps_or_fades() {
    assert_eq!(reveal_alpha(0, 1.0, 0.0), 1.0);
    assert_eq!(reveal_alpha(1, 1.0, 0.0), 0.0);
    assert_eq!(reveal_alpha(1, 1.5, 0.0), 0.0);
    assert!((reveal_alpha(1, 1.5, 1.0) - 0.5).abs() < 1e-6);
    assert_eq!(reveal_alpha(5, 100.0, 2.0), 1.0);
  }
}
```

In `engine.rs`'s tests module, add the reveal-clock tests. They need a layout:

```rust
  #[test]
  fn the_reveal_clock_advances_pauses_and_finishes_once() {
    let (mut engine, _) = engine();
    let text = RichText::markup("ab[pause=1]cd");
    let laid = engine.layout(&text, &TextStyle::default(), &TextLayoutOptions::default(), 1.0).unwrap();
    assert_eq!(laid.pauses, vec![(2, 1.0)]);
    let mut clock = RevealClock::default();
    assert!(!clock.step(0.2, 10.0, 0.0, &laid));
    assert!((clock.shown - 2.0).abs() < 1e-5, "two clusters, then the pause holds");
    assert!(!clock.step(0.5, 10.0, 0.0, &laid));
    assert!((clock.shown - 2.0).abs() < 1e-5, "still paused");
    assert!(clock.step(1.0, 10.0, 0.0, &laid), "finishes this step");
    assert!(clock.done);
    assert!(!clock.step(1.0, 10.0, 0.0, &laid), "finishes once");
  }

  #[test]
  fn punctuation_holds_the_reveal() {
    let (mut engine, _) = engine();
    let laid = engine.layout(&"a, b".into(), &TextStyle::default(), &TextLayoutOptions::default(), 1.0).unwrap();
    assert_eq!(laid.punctuation, vec![1]);
    let mut clock = RevealClock::default();
    clock.step(0.25, 10.0, 0.5, &laid);
    assert!((clock.shown - 2.0).abs() < 1e-5, "a and the comma, then a punctuation pause");
  }
```

In `icons.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn icons_are_found_by_name_and_replaced() {
    let sprite = |name| SpriteHandle::from_sprite(crate::core::graphics::Sprite::solid_rect(4, 4, 255, 0, 0, 255), name);
    let mut icons = TextIcons::default();
    icons.insert("a_button", sprite("a"));
    assert!(icons.get("a_button").is_some_and(|i| i.region.is_none()));
    icons.insert_region("a_button", sprite("sheet"), Rect::new(0.0, 0.0, 2.0, 2.0));
    assert_eq!(icons.get("a_button").and_then(|i| i.region), Some(Rect::new(0.0, 0.0, 2.0, 2.0)));
    icons.remove("a_button");
    assert!(icons.get("a_button").is_none());
  }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib text::`
Expected: compile errors for `markup`, `TextEffects`, `TextIcons`, `RevealClock`.

- [ ] **Step 3: Write `markup.rs`**

```rust
//! The markup syntax: `[tag]`, `[tag=value]`, `[/tag]`, `[[` for a bracket.
//! Style tags: color, size, font, b, i, outline, icon, pause; any other tag
//! is an effect. Malformed markup is drawn literally and warned once.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use crate::core::text::rich::{Icon, IconMark, PauseMark, RichText, Span, SpanMark};
use crate::core::text::style::{Outline, TextFont};
use crate::core::ui::Color;

#[derive(Debug, Clone, PartialEq)]
enum Token<'a> {
  Text(&'a str),
  /// An escaped `[`
  Bracket,
  Open { name: &'a str, value: Option<&'a str>, source: &'a str },
  Close { name: &'a str, source: &'a str },
}

/// The tags that take a value and close (style tags)
const STYLE: [&str; 6] = ["color", "size", "font", "b", "i", "outline"];
/// The tags that stand alone
const SINGLE: [&str; 2] = ["icon", "pause"];

impl RichText {
  /// Parse markup: `"Deal [color=#f44]20[/color] [icon=a_button]"`
  pub fn markup(source: &str) -> Self {
    let tokens = tokenize(source);
    let mut problems = Vec::new();
    let resolved = resolve(&tokens, &mut problems);
    if let Some(problem) = problems.first() {
      warn_once(source, problem);
    }
    build(&resolved)
  }

  /// Hold the typewriter reveal here for `seconds`
  pub fn pause(mut self, seconds: f32) -> Self {
    if seconds.is_finite() && seconds > 0.0 {
      self.pauses.push(PauseMark { at: self.text.len(), seconds });
    }
    self
  }
}

fn tokenize(source: &str) -> Vec<Token<'_>> {
  let mut tokens = Vec::new();
  let mut rest = source;
  while !rest.is_empty() {
    if let Some(after) = rest.strip_prefix("[[") {
      tokens.push(Token::Bracket);
      rest = after;
      continue;
    }
    if rest.starts_with('[') {
      if let Some(end) = rest.find(']') {
        let source = &rest[..=end];
        let inner = &rest[1..end];
        if let Some(name) = inner.strip_prefix('/') {
          tokens.push(Token::Close { name, source });
        } else {
          let (name, value) = match inner.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (inner, None),
          };
          tokens.push(Token::Open { name, value, source });
        }
        rest = &rest[end + 1..];
        continue;
      }
      // A bracket with no end: the rest is text
      tokens.push(Token::Text(rest));
      break;
    }
    let next = rest.find('[').unwrap_or(rest.len());
    tokens.push(Token::Text(&rest[..next]));
    rest = &rest[next..];
  }
  tokens
}
```

Write `resolve` and `build` with this behaviour:

- **`resolve(tokens, problems) -> Vec<Resolved>`** turns each token into a `Resolved::{Text(String), Push(Span), Pop, Icon(String), Pause(f32)}`.
  - **Opens:**
    - A name that is empty or not `[A-Za-z0-9_-]+` is literal (problem: "bad tag").
    - `icon` needs a non-empty value; `pause` needs a finite positive float. Anything else is literal (problem: "bad value").
    - A style tag's value must parse:
      - `color`: `#rgb`, `#rrggbb`, `#rrggbbaa`, or one of `white black red green blue yellow cyan magenta gray grey orange purple pink`;
      - `size`: a finite positive float;
      - `font`: non-empty;
      - `b` and `i`: no value;
      - `outline`: `width,color` with width a finite positive float.

      Otherwise it is literal (problem: "bad value").
    - Any other name is an effect: `Span::effect(source_inner)`, keeping `name=value` whole.
  - **Matching opens with closes** (a stack of open tag names with their token index):
    - A close matches the nearest open of the same name. Any opens above it are closed there too (problem: "mis-nested").
    - A close with no matching open is literal (problem: "close without open").
    - Opens still on the stack at the end are literal: their `Push` becomes `Text(source)` (problem: "unclosed").

    Do the matching as a pre-pass over the token indices, so the decision to make a token literal is known before building.
  - **Bracket** becomes `Text("[")`.
- **`build(&[Resolved]) -> RichText`** keeps a stack of `Span`s, starting with `Span::default()`.
  - `Push(span)` pushes `span.over(top)`.
  - `Pop` pops, unless only the base remains.
  - `Text(t)` calls `rich = rich.span(&t, top.clone())` when the top differs from `Span::default()`, else `rich = rich.plain(&t)`.
  - `Icon(name)` calls `rich = rich.icon_named(name)`.
  - `Pause(s)` calls `rich = rich.pause(s)`.
  - Consecutive `Text`s with equal spans merge into one span, so spans stay few.
- **Colour names:**

```rust
fn parse_color(value: &str) -> Option<Color> {
  let named = match value.to_ascii_lowercase().as_str() {
    "white" => Some(Color::WHITE),
    "black" => Some(Color::BLACK),
    "red" => Some(Color::rgb(255, 0, 0)),
    "green" => Some(Color::rgb(0, 255, 0)),
    "blue" => Some(Color::rgb(0, 0, 255)),
    "yellow" => Some(Color::YELLOW),
    "cyan" => Some(Color::CYAN),
    "magenta" => Some(Color::MAGENTA),
    "gray" | "grey" => Some(Color::GRAY),
    "orange" => Some(Color::rgb(255, 165, 0)),
    "purple" => Some(Color::rgb(128, 0, 128)),
    "pink" => Some(Color::rgb(255, 105, 180)),
    _ => None,
  };
  named.or_else(|| {
    let hex = value.strip_prefix('#')?;
    let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
    let pair = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
      3 => Some(Color::rgb(digit(0)? * 17, digit(1)? * 17, digit(2)? * 17)),
      6 => Some(Color::rgb(pair(0)?, pair(2)?, pair(4)?)),
      8 => Some(Color::new(pair(0)?, pair(2)?, pair(4)?, pair(6)?)),
      _ => None,
    }
  })
}
```

- **Warn once per markup string** (a global set, since parsing is a free function):

```rust
fn warn_once(source: &str, problem: &str) {
  static WARNED: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
  let mut hasher = DefaultHasher::new();
  source.hash(&mut hasher);
  let key = hasher.finish();
  let warned = WARNED.get_or_init(|| Mutex::new(HashSet::new()));
  let first = warned.lock().map(|mut set| set.insert(key)).unwrap_or(false);
  if first {
    crate::engine_warn!(module: "text", "markup {source:?}: {problem}; drawn as written");
  }
}
```

  (`lock()` failing only means a poisoned mutex; it then never warns again, which is acceptable. No `unwrap`.)

`TextFont::Family(value.to_string())` is the `font` tag's override. `Outline { width, color }` is the `outline` tag's.

- [ ] **Step 4: Write `icons.rs`**

```rust
//! Named icons for inline text (`[icon=name]`, `RichText::icon_named`)

use std::collections::HashMap;

use crate::core::asset::SpriteHandle;
use crate::core::ecs::Resource;
use crate::core::ui::Rect;

/// A named icon: a sprite, or a region of one (pixels on the sprite)
#[derive(Debug, Clone, PartialEq)]
pub struct IconSource {
  pub sprite: SpriteHandle,
  pub region: Option<Rect>,
}

/// The icons markup and `RichText::icon_named` can name. Swap a set (say
/// keyboard keys for gamepad buttons) by inserting the same names again.
#[derive(Resource, Debug, Default, Clone)]
pub struct TextIcons {
  icons: HashMap<String, IconSource>,
}

impl TextIcons {
  pub fn insert(&mut self, name: impl Into<String>, sprite: SpriteHandle) {
    self.icons.insert(name.into(), IconSource { sprite, region: None });
  }

  pub fn insert_region(&mut self, name: impl Into<String>, sprite: SpriteHandle, region: Rect) {
    self.icons.insert(name.into(), IconSource { sprite, region: Some(region) });
  }

  pub fn get(&self, name: &str) -> Option<&IconSource> {
    self.icons.get(name)
  }

  pub fn remove(&mut self, name: &str) {
    self.icons.remove(name);
  }
}
```

- [ ] **Step 5: Write `effects.rs`**

```rust
//! Per-glyph effects and the typewriter reveal

use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;
use std::sync::{Arc, Mutex};

use glam::Vec2;

use crate::core::ecs::Resource;
use crate::core::ui::Color;

/// What an effect sees for one glyph
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GlyphContext {
  /// The glyph's index in its text
  pub index: u32,
  /// Its character cluster (the reveal's unit)
  pub cluster: u32,
  pub line: u32,
  /// Seconds: real time in the UI, game time on world text
  pub time: f32,
  /// The text's size in text units
  pub size: f32,
  /// Stable per text, for noise
  pub seed: u64,
  /// The tag's value (`[wave=3,2]` gives `"3,2"`), empty without one
  pub args: String,
}

/// How an effect changes one glyph
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMod {
  /// Text units, +y down
  pub offset: Vec2,
  /// About the glyph's centre
  pub scale: f32,
  /// Replaces the glyph's colour
  pub color: Option<Color>,
  /// Multiplies its opacity
  pub alpha: f32,
}

impl Default for GlyphMod {
  fn default() -> Self {
    Self { offset: Vec2::ZERO, scale: 1.0, color: None, alpha: 1.0 }
  }
}

type EffectFn = Arc<dyn Fn(&GlyphContext) -> GlyphMod + Send + Sync>;

/// The effects text can name. Built in: `wave` (`[wave=amplitude,frequency]`),
/// `shake` (`[shake=intensity]`), `rainbow` (`[rainbow=speed]`).
#[derive(Resource, Clone)]
pub struct TextEffects {
  effects: HashMap<String, EffectFn>,
  warned: Arc<Mutex<HashSet<String>>>,
}

impl std::fmt::Debug for TextEffects {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let mut names: Vec<&String> = self.effects.keys().collect();
    names.sort();
    f.debug_struct("TextEffects").field("effects", &names).finish()
  }
}

impl Default for TextEffects {
  fn default() -> Self {
    let mut effects = Self { effects: HashMap::new(), warned: Arc::default() };
    effects.register("wave", wave);
    effects.register("shake", shake);
    effects.register("rainbow", rainbow);
    effects
  }
}

impl TextEffects {
  /// Add or replace an effect; markup names it `[name]..[/name]`
  pub fn register(&mut self, name: impl Into<String>, effect: impl Fn(&GlyphContext) -> GlyphMod + Send + Sync + 'static) {
    self.effects.insert(name.into(), Arc::new(effect));
  }

  pub fn contains(&self, name: &str) -> bool {
    self.effects.contains_key(name)
  }

  /// Run `effect` (`name` or `name=args`); `None` for an unknown name,
  /// warned once per name
  pub(crate) fn apply(&self, effect: &str, ctx: &GlyphContext) -> Option<GlyphMod> {
    let (name, args) = effect.split_once('=').unwrap_or((effect, ""));
    let Some(run) = self.effects.get(name) else {
      let first = self.warned.lock().map(|mut set| set.insert(name.to_string())).unwrap_or(false);
      if first {
        crate::engine_warn!(module: "text", "no text effect named {name:?}; the text draws without it");
      }
      return None;
    };
    let mut ctx = ctx.clone();
    ctx.args = args.to_string();
    let result = run(&ctx);
    let finite = result.offset.is_finite() && result.scale.is_finite() && result.alpha.is_finite();
    Some(if finite { result } else { GlyphMod::default() })
  }
}

fn args<const N: usize>(ctx: &GlyphContext, defaults: [f32; N]) -> [f32; N] {
  let mut out = defaults;
  for (slot, value) in out.iter_mut().zip(ctx.args.split(',')) {
    if let Ok(v) = value.trim().parse::<f32>()
      && v.is_finite()
    {
      *slot = v;
    }
  }
  out
}

fn wave(ctx: &GlyphContext) -> GlyphMod {
  let [amplitude, frequency] = args(ctx, [ctx.size * 0.15, 2.0]);
  let phase = (ctx.time * frequency + ctx.cluster as f32 * 0.12) * TAU;
  GlyphMod { offset: Vec2::new(0.0, -phase.sin() * amplitude), ..GlyphMod::default() }
}

fn shake(ctx: &GlyphContext) -> GlyphMod {
  let [intensity] = args(ctx, [ctx.size * 0.06]);
  let tick = (ctx.time * 30.0).floor() as u64;
  let noise = |salt: u64| {
    let mut x = ctx.seed ^ (u64::from(ctx.cluster) << 32) ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt;
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    (x as f64 / u64::MAX as f64) as f32 * 2.0 - 1.0
  };
  GlyphMod { offset: Vec2::new(noise(1), noise(2)) * intensity, ..GlyphMod::default() }
}

fn rainbow(ctx: &GlyphContext) -> GlyphMod {
  let [speed] = args(ctx, [0.5]);
  let hue = (ctx.cluster as f32 * 0.08 + ctx.time * speed).rem_euclid(1.0);
  GlyphMod { color: Some(hsv(hue, 0.8, 1.0)), ..GlyphMod::default() }
}

fn hsv(h: f32, s: f32, v: f32) -> Color {
  let i = (h * 6.0).floor();
  let f = h * 6.0 - i;
  let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
  let (r, g, b) = match i as i32 % 6 {
    0 => (v, t, p),
    1 => (q, v, p),
    2 => (p, v, t),
    3 => (p, q, v),
    4 => (t, p, v),
    _ => (v, p, q),
  };
  let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
  Color::rgb(byte(r), byte(g), byte(b))
}

/// Opacity of `cluster` when `shown` clusters are revealed; revealed
/// glyphs fade in over `fade_clusters` clusters (0: they appear at once)
pub(crate) fn reveal_alpha(cluster: u32, shown: f32, fade_clusters: f32) -> f32 {
  let c = cluster as f32;
  if fade_clusters <= 0.0 {
    return if c + 1.0 <= shown { 1.0 } else { 0.0 };
  }
  ((shown - c) / fade_clusters).clamp(0.0, 1.0)
}
```

`reveal_alpha` with fade 0 shows a cluster once it is fully counted (`c + 1 <= shown`). With a fade, it rises from the moment its cluster starts. The test values above pin both.

- [ ] **Step 6: Pauses and punctuation in the layout, and `RevealClock`**

In `layout.rs`, add to `TextLayout`:

```rust
  /// Typewriter pauses: (cluster the pause comes before, seconds)
  pub(crate) pauses: Vec<(u32, f32)>,
  /// Clusters that are punctuation (`. , ! ? ; :` and their full-width forms)
  pub(crate) punctuation: Vec<u32>,
```

In `engine.rs`, while building clusters (Task 2, step 8 of `layout_parley`, and the bitmap layouter):
- each `PauseMark { at, seconds }` becomes `(index of the first cluster starting at or after at, seconds)`;
- each cluster whose first character is in `".,!?;:。、！？；："` is pushed to `punctuation`.

Then add `RevealClock`:

```rust
/// Typewriter state: clusters shown (fractional), seconds of pause left
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct RevealClock {
  pub(crate) shown: f32,
  pub(crate) hold: f32,
  pub(crate) done: bool,
}

impl RevealClock {
  /// Advance by `dt` at `per_second` clusters a second, holding at pauses
  /// and for `punctuation_pause` after punctuation; true on the step the
  /// whole text becomes visible
  pub(crate) fn step(&mut self, dt: f32, per_second: f32, punctuation_pause: f32, layout: &TextLayout) -> bool {
    if self.done {
      return false;
    }
    let total = layout.clusters as f32;
    if !(per_second.is_finite() && per_second > 0.0) || !dt.is_finite() {
      self.shown = total;
      self.done = true;
      return true;
    }
    let mut left = dt.max(0.0);
    while left > 0.0 && self.shown < total {
      if self.hold > 0.0 {
        let spent = self.hold.min(left);
        self.hold -= spent;
        left -= spent;
        continue;
      }
      let next = self.shown.floor() + 1.0;
      let needed = (next - self.shown) / per_second;
      if left < needed {
        self.shown += left * per_second;
        left = 0.0;
      } else {
        self.shown = next;
        left -= needed;
        let revealed = next as u32 - 1;
        if layout.punctuation.contains(&revealed) {
          self.hold += punctuation_pause.max(0.0);
        }
        if let Some(&(_, seconds)) = layout.pauses.iter().find(|(c, _)| *c == next as u32) {
          self.hold += seconds;
        }
      }
    }
    if self.shown >= total {
      self.shown = total;
      self.done = true;
      return true;
    }
    false
  }
}
```

Check it against `the_reveal_clock_advances_pauses_and_finishes_once`. "ab[pause=1]cd" has 4 clusters with a pause before cluster 2:
- 0.2 s at 10/s reveals two clusters, reaches 2.0, and starts a 1 s hold.
- 0.5 s is spent holding.
- The next 1.0 s spends 0.5 s of hold, then reveals two clusters in 0.2 s, and finishes.

Export from `mod.rs`: `mod markup; mod icons; mod effects;`, `pub use icons::{IconSource, TextIcons}; pub use effects::{GlyphContext, GlyphMod, TextEffects}; pub(crate) use effects::reveal_alpha; pub(crate) use engine::RevealClock;`. In `init_world_resources`, insert `TextIcons::default()` and `TextEffects::default()`. Add the public types to the prelude.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p ryo2d --lib text::`
Expected: all pass (Tasks 1-3).

- [ ] **Step 8: AGENTS.md**

Append to `src/core/text/AGENTS.md`:

```markdown
## Markup, icons, effects, reveal

- Markup: `[color=..]`, `[size=..]`, `[font=..]`, `[b]`, `[i]`,
  `[outline=width,color]`, `[icon=name]`, `[pause=seconds]`, `[[` for a
  bracket. Any other tag is an effect (`[wave]`, `[sparkle=3]`). Nested tags
  flatten into non-overlapping spans. Malformed markup (unclosed, unmatched,
  a bad value) is drawn as written and warned once per string; nothing
  panics.
- `TextIcons` maps names to sprites or regions; `TextEffects` maps names to
  `Fn(&GlyphContext) -> GlyphMod` (built in: wave, shake, rainbow). An
  unknown effect name is warned once when drawn and ignored.
- Reveal counts character clusters. `RevealClock` advances them, holding
  at `[pause]` and after punctuation; effects and reveal never relayout.
```

- [ ] **Step 9: Gate and commit**

```bash
git add -A
git commit -m "feat: text markup, icons, effects and typewriter reveal" -m "- markup tags for colour, size, font, bold, italic, outline, icons and pauses; any other tag names an effect
- wave, shake and rainbow built in; games register their own
- malformed markup draws as written and warns once"
```

### Task 4: Glyph rasterization (raster, stroke, pixel, MSDF)

**Files:**
- Create: `src/core/text/raster.rs`, `src/core/text/msdf.rs`
- Modify: `src/core/text/mod.rs`, `src/core/text/AGENTS.md`

**Interfaces:**
- Consumes: `Fonts`, `Face`, `FaceSource`, `FontId`, `FontRender` (Task 1); `TextEngine::coords` (Task 2).
- Produces (crate-internal, GPU-free):
  - `GlyphKey { font: FontId, glyph: u16, size64: u32, subpixel: u8, kind: GlyphKind, coords: u64, skew: bool, embolden: bool }` (`Copy, Eq, Hash`);
  - `GlyphKind::{Fill, Stroke { width64: u32 }, Pixel, Msdf { range64: u32 }}`;
  - `GlyphFormat::{Mask, Color, Msdf}`;
  - `GlyphImage { format, width, height, left: i32, top: i32, data: Vec<u8>, base: f32 }`:
    - `left` is pixels from the pen to the image's left edge;
    - `top` is pixels from the baseline up to the image's top edge;
    - `base` is the pixel size the image was made at;
    - `data` is R8 for `Mask` and RGBA8 for `Color`/`Msdf`;
  - `GlyphRasterizer::{new, rasterize(&mut self, fonts: &Fonts, coords: Option<&[i16]>, key: &GlyphKey) -> Option<GlyphImage>}`;
  - `glyph_id(fonts: &Fonts, font: FontId, c: char) -> Option<u16>`;
  - `raster_size_step(px: f32) -> f32`;
  - `MAX_GLYPH_PX: f32 = 1024.0`;
  - `GlyphKey::fill(font, glyph, px, x_fraction)`, `GlyphKey::stroke(.., width_px)`, `GlyphKey::pixel(..)`, `GlyphKey::msdf(font, glyph, base, range)`.

**Fixed point.** `size64` is the pixel size times 64, rounded; `width64` and `range64` likewise. Keys hash exactly, and sizes within 1/64 px share an image.

- [ ] **Step 1: Write the failing tests**

`raster.rs`, tests first (they use the built-in fonts, so they are `#[cfg(all(test, feature = "default-fonts"))]`):

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::text::{FontRole, Fonts};

  fn ui(fonts: &Fonts) -> FontId {
    fonts.role(FontRole::Ui).unwrap()
  }

  fn fill(fonts: &Fonts, c: char, px: f32) -> GlyphImage {
    let font = ui(fonts);
    let glyph = glyph_id(fonts, font, c).unwrap();
    GlyphRasterizer::new().rasterize(fonts, None, &GlyphKey::fill(font, glyph, px, 0.0)).unwrap()
  }

  #[test]
  fn a_letter_rasterizes_to_a_coverage_mask() {
    let fonts = Fonts::with_builtins();
    let image = fill(&fonts, 'A', 16.0);
    assert_eq!(image.format, GlyphFormat::Mask);
    assert!(image.width > 4 && image.height > 6);
    assert_eq!(image.data.len(), (image.width * image.height) as usize);
    assert!(image.data.iter().any(|&c| c > 200), "solid coverage inside");
    assert!(image.top > 8, "an A stands above the baseline");
  }

  #[test]
  fn a_space_has_no_image() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, ' ').unwrap();
    assert!(GlyphRasterizer::new().rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 16.0, 0.0)).is_none());
  }

  #[test]
  fn subpixel_bins_shift_the_glyph() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'l').unwrap();
    let mut raster = GlyphRasterizer::new();
    let a = raster.rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 16.0, 0.0)).unwrap();
    let b = raster.rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 16.0, 0.5)).unwrap();
    assert!(a.data != b.data || a.left != b.left);
    assert_eq!(GlyphKey::fill(font, glyph, 16.0, 0.6).subpixel, 2);
    assert_eq!(GlyphKey::fill(font, glyph, 16.0, 0.99).subpixel, 0, "rounds up to the next whole pixel");
  }

  #[test]
  fn a_stroke_is_wider_than_the_fill() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'O').unwrap();
    let mut raster = GlyphRasterizer::new();
    let filled = raster.rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 24.0, 0.0)).unwrap();
    let stroked = raster.rasterize(&fonts, None, &GlyphKey::stroke(font, glyph, 24.0, 0.0, 4.0)).unwrap();
    assert!(stroked.width >= filled.width + 3, "{} vs {}", stroked.width, filled.width);
    assert!(stroked.left < filled.left);
  }

  #[test]
  fn pixel_glyphs_are_fully_on_or_off() {
    let fonts = Fonts::with_builtins();
    let font = fonts.role(FontRole::Pixel).unwrap();
    let glyph = glyph_id(&fonts, font, 'K').unwrap();
    let image = GlyphRasterizer::new().rasterize(&fonts, None, &GlyphKey::pixel(font, glyph, 16.0)).unwrap();
    assert!(image.data.iter().all(|&c| c == 0 || c == 255));
    assert!(image.data.iter().any(|&c| c == 255));
  }

  #[test]
  fn an_msdf_glyph_has_an_inside_and_an_outside() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'A').unwrap();
    let image = GlyphRasterizer::new().rasterize(&fonts, None, &GlyphKey::msdf(font, glyph, 32.0, 4.0)).unwrap();
    assert_eq!(image.format, GlyphFormat::Msdf);
    assert_eq!(image.base, 32.0);
    assert_eq!(image.data.len(), (image.width * image.height * 4) as usize);
    let median = |p: &[u8]| {
      let (r, g, b) = (p[0], p[1], p[2]);
      r.min(g).max(r.max(g).min(b))
    };
    let medians: Vec<u8> = image.data.chunks_exact(4).map(median).collect();
    assert!(medians.iter().any(|&m| m > 160), "inside");
    assert!(medians.iter().any(|&m| m < 90), "outside");
  }

  #[test]
  fn synthetic_italic_changes_the_image() {
    let fonts = Fonts::with_builtins();
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'I').unwrap();
    let mut raster = GlyphRasterizer::new();
    let upright = raster.rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 32.0, 0.0)).unwrap();
    let mut key = GlyphKey::fill(font, glyph, 32.0, 0.0);
    key.skew = true;
    let slanted = raster.rasterize(&fonts, None, &key).unwrap();
    assert!(slanted.width > upright.width);
  }

  #[test]
  fn unknown_fonts_and_huge_sizes_give_nothing() {
    let fonts = Fonts::with_builtins();
    let mut raster = GlyphRasterizer::new();
    assert!(raster.rasterize(&fonts, None, &GlyphKey::fill(FontId(999), 36, 16.0, 0.0)).is_none());
    let font = ui(&fonts);
    let glyph = glyph_id(&fonts, font, 'A').unwrap();
    assert!(raster.rasterize(&fonts, None, &GlyphKey::fill(font, glyph, 5000.0, 0.0)).is_none());
  }

  #[test]
  fn raster_sizes_step_as_they_grow() {
    assert_eq!(raster_size_step(10.4), 10.0);
    assert_eq!(raster_size_step(31.6), 32.0);
    assert_eq!(raster_size_step(47.0), 48.0);
    assert_eq!(raster_size_step(63.0), 64.0);
    assert_eq!(raster_size_step(101.0), 100.0);
    assert_eq!(raster_size_step(0.2), 1.0);
  }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib text::raster`
Expected: compile errors (`GlyphKey`, `GlyphRasterizer`, ... not found).

- [ ] **Step 3: Write `raster.rs`**

```rust
//! Glyph bitmaps: swash for raster, stroked and pixel glyphs, fdsm for MSDF.
//! The renderer's glyph atlas stores what this returns.

use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Angle, Format, Join, Stroke, Transform, Vector};
use swash::FontRef;

use crate::core::text::{FaceSource, FontId, Fonts};

/// Glyphs larger than this (pixels) are refused
pub(crate) const MAX_GLYPH_PX: f32 = 1024.0;

/// Sizes above this rasterize unhinted (hinting costs about 10 us a glyph
/// and matters little at large sizes)
const HINT_UP_TO: f32 = 48.0;

/// Subpixel bins across one pixel
const SUBPIXEL_BINS: u8 = 4;

/// What kind of image a glyph key asks for
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GlyphKind {
  Fill,
  /// An outline band `width64 / 64` pixels wide, centred on the outline
  Stroke { width64: u32 },
  /// Coverage thresholded to fully on or off
  Pixel,
  /// A distance field with a `range64 / 64` pixel range at the base size
  Msdf { range64: u32 },
}

/// One glyph image in the atlas
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
  pub(crate) font: FontId,
  pub(crate) glyph: u16,
  /// Pixel size x 64 (the base size for MSDF)
  pub(crate) size64: u32,
  pub(crate) subpixel: u8,
  pub(crate) kind: GlyphKind,
  pub(crate) coords: u64,
  pub(crate) skew: bool,
  pub(crate) embolden: bool,
}

fn fixed(value: f32) -> u32 {
  (value * 64.0).round().clamp(0.0, u32::MAX as f32) as u32
}

impl GlyphKey {
  /// A filled glyph at `px`, its pen at `x_fraction` (0..1) of a pixel
  pub(crate) fn fill(font: FontId, glyph: u16, px: f32, x_fraction: f32) -> Self {
    let bin = (x_fraction.rem_euclid(1.0) * SUBPIXEL_BINS as f32).round() as u8 % SUBPIXEL_BINS;
    Self { font, glyph, size64: fixed(px), subpixel: bin, kind: GlyphKind::Fill, coords: 0, skew: false, embolden: false }
  }

  /// Its outline band, `width_px` wide
  pub(crate) fn stroke(font: FontId, glyph: u16, px: f32, x_fraction: f32, width_px: f32) -> Self {
    Self { kind: GlyphKind::Stroke { width64: fixed(width_px) }, ..Self::fill(font, glyph, px, x_fraction) }
  }

  /// A pixel-font glyph (no subpixel position)
  pub(crate) fn pixel(font: FontId, glyph: u16, px: f32) -> Self {
    Self { subpixel: 0, kind: GlyphKind::Pixel, ..Self::fill(font, glyph, px, 0.0) }
  }

  /// An MSDF glyph made at `base` pixels with a `range` pixel distance range
  pub(crate) fn msdf(font: FontId, glyph: u16, base: f32, range: f32) -> Self {
    Self { subpixel: 0, kind: GlyphKind::Msdf { range64: fixed(range) }, ..Self::fill(font, glyph, base, 0.0) }
  }

  pub(crate) fn px(&self) -> f32 {
    self.size64 as f32 / 64.0
  }
}

/// Which texture kind a glyph image goes in
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GlyphFormat {
  /// R8 coverage
  Mask,
  /// RGBA8, straight alpha, sRGB (colour glyphs)
  Color,
  /// RGBA8 distances, linear
  Msdf,
}

/// A glyph's bitmap and where it sits relative to the pen on the baseline
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlyphImage {
  pub(crate) format: GlyphFormat,
  pub(crate) width: u32,
  pub(crate) height: u32,
  /// Pixels from the pen to the image's left edge
  pub(crate) left: i32,
  /// Pixels from the baseline up to the image's top edge
  pub(crate) top: i32,
  pub(crate) data: Vec<u8>,
  /// The pixel size the image was made at
  pub(crate) base: f32,
}

/// The glyph id of `c` in a font (outline fonts only)
pub(crate) fn glyph_id(fonts: &Fonts, font: FontId, c: char) -> Option<u16> {
  let FaceSource::Outline { data, index } = &fonts.face(font)?.source else {
    return None;
  };
  let font_ref = FontRef::from_index(data, *index as usize)?;
  let id = font_ref.charmap().map(c);
  (id != 0).then_some(id)
}

/// The pixel size raster world text draws at: exact to 32 px, 2 px steps to
/// 64, 4 px steps above, never below 1
pub(crate) fn raster_size_step(px: f32) -> f32 {
  let px = px.max(1.0);
  if px <= 32.0 {
    px.round().max(1.0)
  } else if px <= 64.0 {
    (px / 2.0).round() * 2.0
  } else {
    (px / 4.0).round() * 4.0
  }
}

/// Makes glyph images; owns swash's scaling caches
pub(crate) struct GlyphRasterizer {
  scale: ScaleContext,
}

impl GlyphRasterizer {
  pub(crate) fn new() -> Self {
    Self { scale: ScaleContext::new() }
  }

  /// The image a key names; `None` for an unknown font or glyph, an empty
  /// glyph (a space), a bitmap font (drawn from its sprite instead) or a
  /// size past `MAX_GLYPH_PX`
  pub(crate) fn rasterize(&mut self, fonts: &Fonts, coords: Option<&[i16]>, key: &GlyphKey) -> Option<GlyphImage> {
    let px = key.px();
    if !(px.is_finite() && px > 0.0 && px <= MAX_GLYPH_PX) {
      return None;
    }
    let FaceSource::Outline { data, index } = &fonts.face(key.font)?.source else {
      return None;
    };
    let font = FontRef::from_index(data, *index as usize)?;
    match key.kind {
      GlyphKind::Msdf { range64 } => {
        crate::core::text::msdf::generate(&mut self.scale, font, coords, key.glyph, px, range64 as f32 / 64.0)
      }
      GlyphKind::Fill | GlyphKind::Stroke { .. } | GlyphKind::Pixel => self.swash(font, coords, key, px),
    }
  }

  fn swash(&mut self, font: FontRef<'_>, coords: Option<&[i16]>, key: &GlyphKey, px: f32) -> Option<GlyphImage> {
    let pixel = matches!(key.kind, GlyphKind::Pixel);
    let hint = !pixel && px <= HINT_UP_TO;
    let mut builder = self.scale.builder(font).size(px).hint(hint);
    if let Some(coords) = coords {
      builder = builder.normalized_coords(coords);
    }
    let mut scaler = builder.build();
    let fill_sources = [Source::ColorOutline(0), Source::ColorBitmap(StrikeWith::BestFit), Source::Outline];
    let outline_only = [Source::Outline];
    let mut render = match key.kind {
      GlyphKind::Stroke { .. } => Render::new(&outline_only),
      _ => Render::new(&fill_sources),
    };
    render.format(Format::Alpha);
    render.offset(Vector::new(f32::from(key.subpixel) / f32::from(SUBPIXEL_BINS), 0.0));
    if let GlyphKind::Stroke { width64 } = key.kind {
      render.style(Stroke::new(width64 as f32 / 64.0).join(Join::Round));
    }
    if key.skew {
      render.transform(Some(Transform::skew(Angle::from_degrees(14.0), Angle::ZERO)));
    }
    if key.embolden {
      render.embolden(px * 0.02);
    }
    let image = render.render(&mut scaler, key.glyph)?;
    if image.placement.width == 0 || image.placement.height == 0 {
      return None;
    }
    let (format, mut data) = match image.content {
      Content::Mask => (GlyphFormat::Mask, image.data),
      Content::Color => (GlyphFormat::Color, image.data),
      // Format::Alpha never asks for subpixel masks; treat one as a mask
      Content::SubpixelMask => (GlyphFormat::Mask, image.data.chunks_exact(4).map(|p| p[1]).collect()),
    };
    if pixel && format == GlyphFormat::Mask {
      for coverage in &mut data {
        *coverage = if *coverage >= 128 { 255 } else { 0 };
      }
    }
    Some(GlyphImage {
      format,
      width: image.placement.width,
      height: image.placement.height,
      left: image.placement.left,
      top: image.placement.top,
      data,
      base: px,
    })
  }
}
```

The swash method names here are the spike's (`text-spike/REPORT.md` §3): `Render::new(&SOURCES).format(..).offset(..).style(..).transform(..)`, `embolden(f32)`, `render(&mut scaler, glyph_id) -> Option<Image>`. If `render.style`/`format`/`offset` return `&mut Self` and your code chains them, either is fine.

- [ ] **Step 4: Write `msdf.rs`**

Port the spike's bridge (`text-spike/src/msdf.rs`) without `nalgebra` and without `unwrap`. The points are transformed into image pixels while the shape is built, so no `Shape::transform` and no `nalgebra` dependency is needed:

```rust
//! MSDF glyphs: swash's outline as an fdsm shape, then a distance field

use fdsm::bezier::scanline::FillRule;
use fdsm::bezier::{Point, Segment};
use fdsm::correct_error::{ErrorCorrectionConfig, correct_error_msdf};
use fdsm::generate::generate_msdf;
use fdsm::render::correct_sign_msdf;
use fdsm::shape::{Contour, Shape};
use image::{ImageBuffer, Rgb};
use swash::FontRef;
use swash::scale::ScaleContext;
use swash::zeno::Verb;

use crate::core::text::raster::{GlyphFormat, GlyphImage};

/// An MSDF of `glyph` made at `base` pixels per em with a total distance
/// range of `range` pixels; `None` for an empty glyph
pub(crate) fn generate(
  scale_cx: &mut ScaleContext,
  font: FontRef<'_>,
  coords: Option<&[i16]>,
  glyph: u16,
  base: f32,
  range: f32,
) -> Option<GlyphImage> {
  let units = f32::from(font.metrics(&[]).units_per_em);
  if units <= 0.0 || !(range.is_finite() && range > 0.0) {
    return None;
  }
  let mut builder = scale_cx.builder(font).size(units).hint(false);
  if let Some(coords) = coords {
    builder = builder.normalized_coords(coords);
  }
  let outline = builder.build().scale_outline(glyph)?;
  let bounds = outline.bounds();
  if bounds.is_empty() {
    return None;
  }
  let scale = f64::from(base / units);
  let pad = (f64::from(range) / 2.0).ceil() + 1.0;
  let (x0, y0, x1, y1) = (
    f64::from(bounds.min.x),
    f64::from(bounds.min.y),
    f64::from(bounds.max.x),
    f64::from(bounds.max.y),
  );
  let width = ((x1 - x0) * scale + 2.0 * pad).ceil() as u32;
  let height = ((y1 - y0) * scale + 2.0 * pad).ceil() as u32;
  let ox = pad - x0 * scale;
  let oy = pad + y1 * scale;
  let shape = shape(&outline, |x, y| Point::new(f64::from(x) * scale + ox, oy - f64::from(y) * scale));
  let colored = Shape::edge_coloring_simple(shape, 0.03, 69_441_337_420);
  let prepared = colored.prepare();
  let mut field = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(width, height);
  let range = f64::from(range);
  generate_msdf(&prepared, range, &mut field);
  correct_error_msdf(&mut field, &colored, &prepared, range, &ErrorCorrectionConfig::default());
  correct_sign_msdf(&mut field, &prepared, FillRule::Nonzero);
  let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
  let mut data = Vec::with_capacity((width * height * 4) as usize);
  for pixel in field.pixels() {
    data.extend_from_slice(&[byte(pixel[0]), byte(pixel[1]), byte(pixel[2]), 255]);
  }
  Some(GlyphImage {
    format: GlyphFormat::Msdf,
    width,
    height,
    left: (x0 * scale - pad).floor() as i32,
    top: (y1 * scale + pad).ceil() as i32,
    data,
    base,
  })
}

/// swash's outline (font units, y up) as an fdsm shape, each point mapped
/// by `to_image`
fn shape(outline: &swash::scale::outline::Outline, to_image: impl Fn(f32, f32) -> Point) -> Shape<Contour> {
  let mut shape = Shape::<Contour>::default();
  let points = outline.points();
  let point = |i: usize| points.get(i).map(|p| to_image(p.x, p.y));
  let mut next = 0;
  let mut start = Point::new(0.0, 0.0);
  let mut last = start;
  let mut open = false;
  let mut push = |shape: &mut Shape<Contour>, segment: Segment| {
    if let Some(contour) = shape.contours.last_mut() {
      contour.segments.push(segment);
    }
  };
  for verb in outline.verbs() {
    match verb {
      Verb::MoveTo => {
        if open && (last - start).norm() > 1e-9 {
          push(&mut shape, Segment::line(last, start));
        }
        let Some(p) = point(next) else { break };
        next += 1;
        start = p;
        last = p;
        shape.contours.push(Contour::default());
        open = true;
      }
      Verb::LineTo => {
        let Some(p) = point(next) else { break };
        next += 1;
        push(&mut shape, Segment::line(last, p));
        last = p;
      }
      Verb::QuadTo => {
        let (Some(c), Some(p)) = (point(next), point(next + 1)) else { break };
        next += 2;
        push(&mut shape, Segment::quad(last, c, p));
        last = p;
      }
      Verb::CurveTo => {
        let (Some(c1), Some(c2), Some(p)) = (point(next), point(next + 1), point(next + 2)) else { break };
        next += 3;
        push(&mut shape, Segment::cubic(last, c1, c2, p));
        last = p;
      }
      Verb::Close => {
        if (last - start).norm() > 1e-9 {
          push(&mut shape, Segment::line(last, start));
        }
        last = start;
        open = false;
      }
    }
  }
  if open && (last - start).norm() > 1e-9 {
    push(&mut shape, Segment::line(last, start));
  }
  shape
}
```

`left`/`top` follow swash's convention so the emitter places both kinds the same way. The image's left edge is `pad` pixels left of the outline's left, and its top edge is `pad` pixels above the outline's top. The emitter divides them by `base` and multiplies by the drawn size.

If the borrow checker rejects the `push` closure next to `shape.contours.push`, make `push` a plain `fn push(shape: &mut Shape<Contour>, segment: Segment)`.

`outline.bounds()` returns a `zeno::Bounds`; check the field names (`min`, `max`, `is_empty`) on docs.rs. If `Bounds` lacks `is_empty`, use `width() <= 0.0 || height() <= 0.0`.

Register in `mod.rs`: `mod msdf; mod raster; pub(crate) use raster::{GlyphFormat, GlyphImage, GlyphKey, GlyphKind, GlyphRasterizer, MAX_GLYPH_PX, glyph_id, raster_size_step};`.

Add a guard test in `src/core/text/mod.rs`, written like `bind_groups.rs`'s `every_bind_group_is_created_through_the_counter`:

```rust
#[cfg(test)]
mod guard {
  /// parley, swash and fdsm stay inside src/core/text/
  #[test]
  fn the_text_crates_are_named_only_in_the_text_module() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
      for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
          stack.push(path);
        } else if path.extension().is_some_and(|e| e == "rs") && !path.starts_with(root.join("core/text")) {
          let source = std::fs::read_to_string(&path).unwrap();
          for name in ["parley::", "swash::", "fdsm::"] {
            if source.contains(name) {
              offenders.push(format!("{} names {name}", path.display()));
            }
          }
        }
      }
    }
    assert!(offenders.is_empty(), "{offenders:#?}");
  }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ryo2d --lib text::raster`
Expected: 9 passed.

Then time it once: add a temporary `#[test] #[ignore]` that rasterizes 100 MSDF glyphs, and run it with `--release --ignored --nocapture`. Record the per-glyph time in the report (the spike measured 337 µs with error correction), then delete the temporary test.

- [ ] **Step 6: AGENTS.md**

Append:

```markdown
## Glyph images

- `GlyphKey` names one image: font, glyph id, size (1/64 px fixed point),
  a 4-bin subpixel position (raster fills and strokes only), kind (fill,
  stroke band, pixel, MSDF), variable-font coordinates, synthesized italic
  (a 14 degree skew) and bold.
- Raster glyphs are hinted up to 48 px, unhinted above; pixel glyphs are
  thresholded to on/off; strokes are a band centred on the outline (drawn
  under the fill). MSDF glyphs are made once per glyph at the font's base
  size (default 32 px, range 4 px) with error correction, about a third of
  a millisecond each: `Fonts::prewarm` builds known text ahead.
- Glyphs above 1024 px are refused. Bitmap fonts have no glyph images:
  the renderer draws their cells from the sprite sheet.
```

- [ ] **Step 7: Gate and commit**

```bash
git add -A
git commit -m "feat: glyph rasterization" -m "- hinted raster glyphs with subpixel positions, outline bands and pixel-font thresholding
- MSDF glyphs from the font outlines, with error correction"
```

### Task 5: The glyph atlas

**Files:**
- Create: `src/core/graphics/glyph_atlas.rs`
- Modify:
  - `src/core/graphics/atlas.rs`:
    - `SlotPage::Glyph(GlyphPage)`;
    - `PageBindGroups` becomes `pub(crate)`;
    - `TextureAtlas` owns a `GlyphAtlas`, with `bind_group` and `texture` arms;
    - `TextureAtlas::glyph(..)`.
  - `src/core/graphics/sprite_gpu.rs`: `SpriteGpuRenderer::glyph(..)`, `frame()`.
  - `src/core/graphics/mod.rs`.
  - `src/core/render/batch/key.rs`: the `PageKind::of` arm.
  - `src/core/testing/gpu_world.rs`: a GPU check.

**Interfaces:**
- Consumes: `GlyphKey`, `GlyphImage`, `GlyphFormat`, `MAX_GLYPH_PX` (Task 4).
- Produces (crate-internal):
  - `GlyphPage { format: GlyphFormat, index: u16 }`;
  - `GlyphSlot { page: GlyphPage, uv: [f32; 4], width: u32, height: u32, left: i32, top: i32, base: f32 }`;
  - `SlotPage::Glyph(GlyphPage)`;
  - `SpriteGpuRenderer::glyph(&mut self, device, queue, key: GlyphKey, raster: impl FnOnce() -> Option<GlyphImage>) -> Option<GlyphSlot>`;
  - `SpriteGpuRenderer::frame(&self) -> u64`;
  - `SpriteGpuRenderer::page_bind_group(SlotPage::Glyph(..), sampling)`, which works through the existing seam.

**Pages:**
- 1024×1024, up to 8 per format:
  - `Mask`: `R8Unorm`.
  - `Color`: `Rgba8Unorm` holding sRGB bytes, like the sprite pages.
  - `Msdf`: `Rgba8Unorm` holding linear distances.
- No mips.
- Every glyph gets a 1 px transparent border so linear sampling never reaches a neighbour.
- Bind groups (nearest and linear) are made when a page is made, through `TextureAtlas::bind`.

- [ ] **Step 1: Write the failing tests**

The packing and eviction bookkeeping is a CPU struct, `GlyphShelves`, so it is unit-tested without a GPU. In `glyph_atlas.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::text::{FontId, GlyphFormat, GlyphKey};

  fn key(glyph: u16) -> GlyphKey {
    GlyphKey::fill(FontId(0), glyph, 16.0, 0.0)
  }

  #[test]
  fn a_placed_glyph_is_found_again() {
    let mut shelves = GlyphShelves::new(64, 2);
    let placed = shelves.place(&key(1), GlyphFormat::Mask, (10, 12), 0);
    let Placement::Placed { page, rect } = placed else { panic!("{placed:?}") };
    assert_eq!(page, 0);
    assert_eq!((rect.width, rect.height), (10, 12));
    assert_eq!(shelves.lookup(&key(1), 1).map(|e| e.rect), Some(rect));
    assert!(shelves.lookup(&key(2), 1).is_none());
  }

  #[test]
  fn a_new_page_is_asked_for_until_the_cap() {
    let mut shelves = GlyphShelves::new(32, 2);
    let mut asked = 0;
    for glyph in 0..40 {
      match shelves.place(&key(glyph), GlyphFormat::Mask, (12, 12), 0) {
        Placement::NeedPage => {
          asked += 1;
          shelves.add_page(GlyphFormat::Mask);
          assert!(matches!(shelves.place(&key(glyph), GlyphFormat::Mask, (12, 12), 0), Placement::Placed { .. }));
        }
        Placement::Placed { .. } => {}
        Placement::Full => break,
        Placement::TooBig => panic!("not too big"),
      }
    }
    assert_eq!(asked, 2, "one page per format to start, then one more, up to two");
  }

  #[test]
  fn a_glyph_larger_than_a_page_is_refused_once() {
    let mut shelves = GlyphShelves::new(64, 2);
    assert!(matches!(shelves.place(&key(9), GlyphFormat::Mask, (100, 10), 0), Placement::TooBig));
    assert!(matches!(shelves.place(&key(9), GlyphFormat::Mask, (100, 10), 1), Placement::TooBig));
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("glyph") && e.message.contains("100x10"))
      .count();
    assert_eq!(warnings, 1);
  }

  #[test]
  fn a_full_atlas_keeps_this_frames_glyphs() {
    let mut shelves = GlyphShelves::new(32, 1);
    shelves.add_page(GlyphFormat::Mask);
    let mut placed = Vec::new();
    for glyph in 0.. {
      match shelves.place(&key(glyph), GlyphFormat::Mask, (12, 12), 5) {
        Placement::Placed { .. } => placed.push(glyph),
        Placement::Full => break,
        other => panic!("{other:?}"),
      }
    }
    assert!(!placed.is_empty());
    // Same frame: nothing drawn this frame is evicted
    assert!(matches!(shelves.place(&key(1000), GlyphFormat::Mask, (12, 12), 5), Placement::Full));
    for glyph in &placed {
      assert!(shelves.lookup(&key(*glyph), 5).is_some());
    }
    // A later frame: the least recently used page is cleared for it
    assert!(matches!(shelves.place(&key(1000), GlyphFormat::Mask, (12, 12), 6), Placement::Placed { .. }));
    assert!(shelves.lookup(&key(placed[0]), 6).is_none());
  }

  #[test]
  fn formats_keep_separate_pages() {
    let mut shelves = GlyphShelves::new(64, 2);
    shelves.add_page(GlyphFormat::Mask);
    shelves.add_page(GlyphFormat::Msdf);
    let Placement::Placed { page: mask, .. } = shelves.place(&key(1), GlyphFormat::Mask, (8, 8), 0) else { panic!() };
    let Placement::Placed { page: msdf, .. } = shelves.place(&key(2), GlyphFormat::Msdf, (8, 8), 0) else { panic!() };
    assert_eq!((mask, msdf), (0, 0), "index within each format");
    assert_eq!(shelves.lookup(&key(2), 0).map(|e| e.format), Some(GlyphFormat::Msdf));
  }
}
```

In `src/core/testing/gpu_world.rs`, add a GPU check beside `steady_frames_create_no_bind_groups`. It uses the shared device the other checks use; look at how they get it (`shared_gpu()`) and construct a `SpriteGpuRenderer`:

```rust
#[test]
fn glyph_pages_bind_once_and_reuse_their_glyphs() {
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    eprintln!("skipping: no GPU available");
    return;
  };
  let mut sprites = crate::core::graphics::SpriteGpuRenderer::new(&device, &queue);
  let image = |w: u32, h: u32| crate::core::text::GlyphImage {
    format: crate::core::text::GlyphFormat::Mask,
    width: w,
    height: h,
    left: 0,
    top: h as i32,
    data: vec![255; (w * h) as usize],
    base: 16.0,
  };
  let before = crate::core::graphics::bind_groups::created_on_this_thread();
  let a = crate::core::text::GlyphKey::fill(crate::core::text::FontId(0), 1, 16.0, 0.0);
  let b = crate::core::text::GlyphKey::fill(crate::core::text::FontId(0), 2, 16.0, 0.0);
  let slot_a = sprites.glyph(&device, &queue, a, || Some(image(8, 10))).expect("placed");
  let _ = sprites.glyph(&device, &queue, b, || Some(image(6, 9))).expect("placed");
  let made = crate::core::graphics::bind_groups::created_on_this_thread() - before;
  assert_eq!(made, 2, "one page, bound once per sampler");
  let again = sprites.glyph(&device, &queue, a, || panic!("no second raster")).expect("cached");
  assert_eq!(again, slot_a);
  assert!(sprites.page_bind_group(crate::core::graphics::SlotPage::Glyph(slot_a.page), crate::core::render::batch::Sampling::Linear).is_some());
}
```

(`shared_gpu()` returns `(Arc<Device>, Arc<Queue>)`. Adjust the module paths to what `gpu_world.rs` already imports. The `panic!` inside the closure is test code: it asserts the cached path never calls the rasterizer.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib glyph_atlas` and `cargo test -p ryo2d --lib glyph_pages_bind_once`
Expected: compile errors.

- [ ] **Step 3: Write `glyph_atlas.rs`**

```rust
//! Glyph images in their own pages beside the sprite atlas: R8 coverage
//! (raster glyphs), RGBA (colour glyphs, sRGB bytes) and RGBA distances
//! (MSDF). Glyphs are placed on first use and evicted a page at a time,
//! least recently used first, never one drawn this frame.

use std::collections::{HashMap, HashSet};

use etagere::{AllocId, BucketedAtlasAllocator, size2};

use crate::core::graphics::atlas::PageBindGroups;
use crate::core::render::batch::Sampling;
use crate::core::text::{GlyphFormat, GlyphImage, GlyphKey};

/// Page edge in texels
pub(crate) const GLYPH_PAGE_SIZE: u32 = 1024;
/// Pages per format
pub(crate) const GLYPH_PAGES: usize = 8;
/// Transparent texels around each glyph
const PADDING: u32 = 1;

/// A glyph page: its format and index within that format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GlyphPage {
  pub(crate) format: GlyphFormat,
  pub(crate) index: u16,
}

/// Where a glyph's image is and how it sits on the pen
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GlyphSlot {
  pub(crate) page: GlyphPage,
  /// u_min, v_min, u_max, v_max of the image (inside its padding)
  pub(crate) uv: [f32; 4],
  pub(crate) width: u32,
  pub(crate) height: u32,
  pub(crate) left: i32,
  pub(crate) top: i32,
  /// The pixel size the image was made at
  pub(crate) base: f32,
}

/// A placed image's texel rectangle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TexelRect {
  pub(crate) x: u32,
  pub(crate) y: u32,
  pub(crate) width: u32,
  pub(crate) height: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct ShelfEntry {
  pub(crate) format: GlyphFormat,
  pub(crate) page: usize,
  pub(crate) rect: TexelRect,
  alloc: AllocId,
  last_used: u64,
}

#[derive(Debug)]
pub(crate) enum Placement {
  Placed { page: usize, rect: TexelRect },
  /// No page of this format has room and another may be made
  NeedPage,
  /// Every page holds glyphs drawn this frame
  Full,
  /// Bigger than a page
  TooBig,
}

struct ShelfPage {
  allocator: BucketedAtlasAllocator,
  keys: HashSet<GlyphKey>,
}

/// Packing and eviction, GPU-free
pub(crate) struct GlyphShelves {
  size: u32,
  max_pages: usize,
  pages: HashMap<GlyphFormat, Vec<ShelfPage>>,
  entries: HashMap<GlyphKey, ShelfEntry>,
  warned: HashSet<(u32, u32)>,
  warned_full: bool,
}
```

Then implement these `GlyphShelves` methods:

- **`new(size, max_pages)`.**
- **`lookup(&mut self, key, frame) -> Option<&ShelfEntry>`:** sets `last_used = frame` and returns the entry.
- **`add_page(&mut self, format)`:** pushes a `ShelfPage` with `BucketedAtlasAllocator::new(size2(size as i32, size as i32))`.
- **`place(&mut self, key, format, (w, h), frame) -> Placement`:**
  1. Let `(pw, ph) = (w + 2 * PADDING, h + 2 * PADDING)`. If either exceeds `size`, return `TooBig`. Warn once per `(w, h)` with `engine_warn!(module: "text", "a {w}x{h} glyph is larger than a glyph page; it is not drawn")`.
  2. Try `allocator.allocate(size2(pw as i32, ph as i32))` on each page of `format`. On success, record the entry: `rect` is the requested image's texels, `x = alloc.rectangle.min.x as u32 + PADDING`, same for `y`, with `width = w`, `height = h`. Do not use the allocator's rounded-up rectangle size. Then return `Placed`.
  3. If no page has room and `pages.len() < max_pages`, return `NeedPage`. The caller makes the GPU page, calls `add_page`, and calls `place` again.
  4. Otherwise pick the page of this format whose newest `last_used` among its keys is oldest and `< frame`. Clear it: `allocator.clear()`, and remove its keys from `entries`. Then allocate there and return `Placed`.
  5. If every page has a key used this frame, return `Full`. Warn once with `engine_warn!(module: "text", "the glyph atlas is full of glyphs drawn this frame; some text is not drawn")`, and reset `warned_full` when a later `place` succeeds.

Then the GPU side:

```rust
struct GpuPage {
  texture: wgpu::Texture,
  bind_groups: PageBindGroups,
}

/// The glyph pages on the GPU
pub(crate) struct GlyphAtlas {
  shelves: GlyphShelves,
  pages: HashMap<GlyphFormat, Vec<GpuPage>>,
}
```

and these `GlyphAtlas` methods:

- **`new()`:** `GlyphShelves::new(GLYPH_PAGE_SIZE, GLYPH_PAGES)` and no pages.
- **`slot(&mut self, key, frame) -> Option<GlyphSlot>`:** looks the key up, touches it, and builds the `GlyphSlot` from the entry: `uv = [x / S, y / S, (x + w) / S, (y + h) / S]` with `S = GLYPH_PAGE_SIZE`, plus the `left`, `top` and `base` kept with the entry. Store those three in `ShelfEntry` too, set by `insert` below.
- **`insert(&mut self, device, queue, key, image: &GlyphImage, frame, bind: impl Fn(&wgpu::TextureView) -> PageBindGroups) -> Option<GlyphSlot>`:**
  1. `place`. On `NeedPage`, create the texture (`TEXTURE_BINDING | COPY_DST`, 1 mip, format `R8Unorm` for `Mask`, `Rgba8Unorm` otherwise, label `"Glyph Page"`), its bind groups through `bind(&view)`, `shelves.add_page`, then `place` again. On `Full` or `TooBig`, return `None`.
  2. Write the image with one padded write: a `(w + 2) x (h + 2)` buffer of zeros with the image copied in at (1, 1), sent by `queue.write_texture` to `(x - 1, y - 1)`. `bytes_per_row` is `(w + 2) * bpp`, where bpp is 1 for `Mask` and 4 otherwise. `write_texture` has no 256-byte row alignment rule (only buffer-to-texture copies do).
  3. Store `left`, `top` and `base` in the entry and return its slot.
- **`bind_group(&self, page: GlyphPage, sampling) -> Option<&wgpu::BindGroup>`.**
- **`texture(&self, page: GlyphPage) -> Option<&wgpu::Texture>`.**

In `atlas.rs`:
- add `Glyph(crate::core::graphics::glyph_atlas::GlyphPage)` to `SlotPage`;
- make `PageBindGroups` and its fields `pub(crate)`;
- give `TextureAtlas` a `glyphs: GlyphAtlas` field;
- add `SlotPage::Glyph(page) => return self.glyphs.bind_group(page, sampling)` to `bind_group`, and the `texture` equivalent;
- add:

```rust
  /// The slot of a glyph image, rasterizing and uploading it on first use
  pub(crate) fn glyph(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    key: GlyphKey,
    frame: u64,
    raster: impl FnOnce() -> Option<GlyphImage>,
  ) -> Option<GlyphSlot> {
    if let Some(slot) = self.glyphs.slot(&key, frame) {
      return Some(slot);
    }
    let image = raster()?;
    let (layout, nearest, linear) = (&self.bind_layout, &self.nearest, &self.linear);
    self.glyphs.insert(device, queue, key, &image, frame, |view| {
      bind_with(device, layout, view, nearest, linear, "Glyph Page")
    })
  }
```

Factor the body of the existing `TextureAtlas::bind` into a free function `bind_with(device, layout, view, nearest, linear, label) -> PageBindGroups`. Have `bind` call it, so the borrow of `self.glyphs` and the samplers don't conflict.

In `key.rs`, `PageKind::of` keeps `Srgb8` for `SlotPage::Glyph(_)`; text shaders do not use `PageKind`. In `sprite_gpu.rs`, add:

```rust
  /// The current frame number (the atlas's least-recently-used clock)
  pub(crate) fn frame(&self) -> u64 {
    self.frame
  }

  /// A glyph image's slot, rasterizing and uploading it on first use
  pub(crate) fn glyph(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    key: GlyphKey,
    raster: impl FnOnce() -> Option<GlyphImage>,
  ) -> Option<GlyphSlot> {
    self.atlas.glyph(device, queue, key, self.frame, raster)
  }
```

`page_bind_group` already delegates to `atlas.bind_group`, so glyph pages work through it unchanged.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib glyph_atlas glyph_pages_bind_once`
Expected: 6 passed (the GPU test skips without a GPU; it must pass on this machine).
Also run `cargo test -p ryo2d --lib steady_frames_create_no_bind_groups`. Expected: still passes.

- [ ] **Step 5: Docs**

Add a "Glyph pages" bullet to `src/core/graphics/AGENTS.md` under "Texture Atlas and Shading". It covers:
- the three formats, 1024² pages, up to 8 per format, no mips, and the 1 px padding;
- placement on first use;
- eviction a whole page at a time, least recently used, never one drawn this frame;
- a glyph bigger than a page is refused and warned once;
- bind groups are made with the page.

- [ ] **Step 6: Gate and commit**

```bash
git add -A
git commit -m "feat: glyph atlas pages" -m "- coverage, colour and MSDF glyph pages beside the sprite atlas, bound once each
- glyphs placed on first use; pages evicted least recently used, never one drawn this frame"
```

### Task 6: Batch support (glyph and shape items, text shaders, scissor, per-camera format)

**Files:**
- Create: `src/core/shaders/core/text.wgsl`
- Modify:
  - `src/core/shaders/compose.rs`: `SPRITE` gains `text.wgsl`.
  - `src/core/render/batch/key.rs`: `BatchShader::{TextMask, TextColor, TextMsdf, UiRect}`.
  - `src/core/render/batch/pipelines.rs`: `ensure` rows.
  - `src/core/render/batch/gpu.rs`:
    - `place` arms for glyphs and shapes;
    - per-camera format;
    - scissor;
    - untextured `UiRect` quads;
    - `draw` takes the image size.
  - `src/core/render/draw_list.rs`:
    - `DrawKind::Text`;
    - `DrawPayload::{Glyph, Shape}`;
    - `DrawGlyph`, `DrawShape`;
    - `scissor` on `DrawSprite` and `DrawTriangles`;
    - `push_glyph`, `push_shape`;
    - UI cameras in `sees`.
  - `src/core/render/frame.rs`: `CameraDraw::ui`.
  - `src/core/render/context.rs`: `RenderContext::camera_draw()`.
  - `src/core/render/passes/world.rs`: passes the image size to `draw`.
  - `src/core/testing/gpu_world.rs`: a pipeline check.

**Interfaces:**
- Consumes: `GlyphPage`, `GlyphSlot`, `SlotPage::Glyph` (Task 5).
- Produces (crate-internal):
  - `DrawKind::{Entity, Text, Sprite, Trail}`: Text sorts after entities at equal z.
  - `DrawGlyph { page: GlyphPage, instance: SpriteInstance, shader: BatchShader, blend: BlendMode, sampling: Sampling, layers: RenderLayers, min: Vec2, max: Vec2, scissor: Option<PixelRect> }`.
  - `DrawShape { instance: SpriteInstance, blend: BlendMode, layers: RenderLayers, min: Vec2, max: Vec2, scissor: Option<PixelRect> }`.
  - `DrawList::push_glyph(&mut self, kind: DrawKind, z: i32, lit: bool, only_camera: Option<Entity>, glyph: DrawGlyph)`.
  - `DrawList::push_shape(&mut self, kind: DrawKind, z: i32, lit: bool, only_camera: Option<Entity>, shape: DrawShape)`.
  - `DrawSprite::scissor` and `DrawTriangles::scissor`, both `Option<PixelRect>` (every current producer sets `None`).
  - `CameraDraw::ui: bool`: a UI camera sees only items drawn for it, and draws in `UI_FORMAT`.
  - `pub(crate) const UI_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm` (in `render/mod.rs`): the UI image holds gamma-encoded premultiplied colour (Plan Notes).
  - The WGSL override constant `UI_GAMMA` (in `include/color.wgsl`, default `false`) and `fn ui_out(c: vec4<f32>) -> vec4<f32>`. Every sprite, line and text fragment entry returns through `ui_out`, which re-encodes premultiplied linear colour to premultiplied sRGB when `UI_GAMMA` is set. Pipelines built for `UI_FORMAT` set it to `true`.
  - `Batch2D::draw(&'a self, pass, draw: &Draw, camera: Entity, textures: &'a SpriteGpuRenderer, image: (u32, u32))`.

**Instance lanes used by the new shaders:**

| Shader | `tint` | `effect_params` | `effect_extra` |
|---|---|---|---|
| `TextMask` | glyph colour (sRGB, straight alpha) | unused | unused |
| `TextColor` | multiplier (white keeps the glyph's colours) | unused | unused |
| `TextMsdf` | fill colour, or the soft layer's colour | `[mode (0 fill/outline, 1 soft), width or spread (base px), softness (base px), range (base px)]` | outline colour |
| `UiRect` | fill colour | `[corner radius, border width, width, height]` (UI units) | border colour |

`uv_rect` is the glyph's atlas UV (`[0, 0, 1, 1]` for `UiRect`). `uv_bounds` is set equal to `uv_rect`.

- [ ] **Step 1: Write the failing tests**

In `draw_list.rs`'s tests module (pure, no GPU), add these. Use the module's existing helpers for a `CameraDraw` and a `SpriteInstance`; read the module first. `camera(..)` below stands for whatever helper builds a test camera, extended with `ui`.

```rust
  fn glyph_item(min: Vec2, max: Vec2) -> DrawGlyph {
    DrawGlyph {
      page: GlyphPage { format: GlyphFormat::Mask, index: 0 },
      instance: SpriteInstance::new(),
      shader: BatchShader::TextMask,
      blend: BlendMode::Alpha,
      sampling: Sampling::Linear,
      layers: RenderLayers::default(),
      min,
      max,
      scissor: None,
    }
  }

  #[test]
  fn text_sorts_after_entities_at_equal_z() {
    let mut list = DrawList::default();
    list.push_glyph(DrawKind::Text, 0, true, None, glyph_item(Vec2::ZERO, Vec2::ONE));
    push_test_entity_sprite(&mut list, 0);
    let list = list.sorted();
    let kinds: Vec<DrawKind> = list.items().iter().map(|i| i.kind).collect();
    assert_eq!(kinds, vec![DrawKind::Entity, DrawKind::Text]);
  }

  #[test]
  fn a_ui_camera_sees_only_its_own_items() {
    let ui = Entity::from_raw_u32(900).unwrap();
    let world_camera = test_camera(Entity::from_raw_u32(1).unwrap(), false);
    let ui_camera = test_camera(ui, true);
    let mut list = DrawList::default();
    list.push_glyph(DrawKind::Text, 0, false, Some(ui), glyph_item(Vec2::ZERO, Vec2::ONE));
    list.push_glyph(DrawKind::Text, 0, false, None, glyph_item(Vec2::ZERO, Vec2::ONE));
    let list = list.sorted();
    assert_eq!(list.unlit_items_for(&ui_camera).count(), 1);
    assert_eq!(list.unlit_items_for(&world_camera).count(), 1);
  }
```

(`push_test_entity_sprite` and `test_camera` stand for the existing helpers. Name them as the module does, or add minimal ones. `Entity::from_raw_u32` is bevy 0.20's constructor; use what the module already uses to make test entities.)

In `plan.rs`'s tests (they already have `state`, `list`, `plan` and `quads` helpers), add:

```rust
  #[test]
  fn a_scissor_change_splits_draws() {
    let clip = Some(crate::core::render::PixelRect { x: 0, y: 0, width: 10, height: 10 });
    let open = DrawState { page: None, shader: BatchShader::UiRect, ..state(0, BlendMode::Alpha) };
    let clipped = DrawState { scissor: clip, ..open };
    let states = [open, open, clipped, clipped, open];
    let out = plan(&list(5), |i| Some(Placed::Quad(states[i])));
    assert_eq!(quads(&out), [(open, 0..2), (clipped, 2..4), (open, 4..5)]);
  }
```

(`PixelRect` lives in `render/frame.rs`; use the path the module can see.)

In `src/core/testing/gpu_world.rs`, add a GPU check that every new pipeline compiles in both formats. First add a test-only helper on `Batch2D` (`gpu.rs`):

```rust
  /// Build one pipeline and say whether it exists (tests)
  #[cfg(test)]
  pub(crate) fn ensure_pipeline_for_test(
    &mut self,
    device: &wgpu::Device,
    shader: BatchShader,
    blend: BlendMode,
    format: wgpu::TextureFormat,
  ) -> bool {
    let key = PipelineKey { shader, blend, stream: Stream::Quads, page: PageKind::Srgb8, format };
    self.pipelines.ensure(device, key, None);
    self.pipelines.get(&key).is_some()
  }
```

Then the check:

```rust
#[test]
fn the_text_and_ui_pipelines_build_in_both_formats() {
  use crate::core::render::batch::{BatchShader, BlendMode};
  use crate::core::render::{SCENE_FORMAT, UI_FORMAT};
  let Ok((device, _queue)) = crate::core::testing::shared_gpu() else {
    eprintln!("skipping: no GPU available");
    return;
  };
  let mut batch = crate::core::render::batch::Batch2D::new(&device, SCENE_FORMAT, Default::default());
  device.push_error_scope(wgpu::ErrorFilter::Validation);
  for shader in [BatchShader::TextMask, BatchShader::TextColor, BatchShader::TextMsdf, BatchShader::UiRect] {
    for blend in [BlendMode::Alpha, BlendMode::Premultiplied] {
      for format in [SCENE_FORMAT, UI_FORMAT] {
        assert!(
          batch.ensure_pipeline_for_test(&device, shader, blend, format),
          "{shader:?} {blend:?} {format:?}"
        );
      }
    }
  }
  let error = pollster::block_on(device.pop_error_scope());
  assert!(error.is_none(), "{error:?}");
}
```

(`Batch2D::new`'s third argument is a `Presentation`; `Default::default()` is `ResolutionIndependent`. `pop_error_scope` returns a future of `Option<wgpu::Error>`; if wgpu 30 returns a `Result` instead, assert on that.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib draw_list plan:: the_text_and_ui_pipelines_build`
Expected: compile errors.

- [ ] **Step 3: The draw list**

In `draw_list.rs`:
- add `Text` to `DrawKind`, between `Entity` and `Sprite`;
- add `Glyph(usize)` and `Shape(usize)` to `DrawPayload`;
- add `glyphs: Vec<DrawGlyph>` and `shapes: Vec<DrawShape>` to `DrawList`, cleared in `clear()`;
- add the two structs as in Interfaces, with doc comments;
- add `pub(crate) scissor: Option<PixelRect>` to `DrawSprite` and `DrawTriangles`, and set `scissor: None` at every existing construction (`push_sprite_in`'s caller sites, `push_polyline`).

Write `push_glyph` and `push_shape` like `push_triangles`: push the payload, then `push_item(lit, z, kind, payload, only_camera)`.

In `sees`:
- an item for another camera is not seen (as now);
- a UI camera sees only items whose `only_camera` is itself;
- glyphs and shapes are seen like sprites: layers intersect and `camera.view.is_visible(min, max)`.

```rust
    if camera.ui && item.only_camera != Some(camera.entity) {
      return false;
    }
```

Add accessors `glyph(i) -> &DrawGlyph` and `shape(i) -> &DrawShape`, as `sprite(i)` exists. `triangle_textures()` is unchanged: glyph pages are not registry textures.

In `frame.rs`, add `pub(crate) ui: bool` to `CameraDraw` (`false` in `plan_cameras`). In `render/mod.rs`, add `UI_FORMAT`.

- [ ] **Step 4: The shaders**

Create `src/core/shaders/core/text.wgsl`. It is composed after `sprite.wgsl`, so it uses that file's `VertexOutput`, `sprite_texture`, `sprite_sampler` and `color.wgsl`'s `srgb_to_linear`:

```wgsl
// Text and UI fragment entries: composed after sprite.wgsl (its vertex
// stage, VertexOutput and group 1 texture)

// A raster glyph: tint times coverage, premultiplied
@fragment
fn fs_text_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    let coverage = textureSample(sprite_texture, sprite_sampler, in.uv).r;
    let a = coverage * in.tint.a;
    return vec4<f32>(srgb_to_linear(in.tint.rgb) * a * in.emission, a);
}

// A colour glyph (sRGB bytes, straight alpha) times the tint
@fragment
fn fs_text_color(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(sprite_texture, sprite_sampler, in.uv);
    let a = texel.a * in.tint.a;
    return vec4<f32>(srgb_to_linear(texel.rgb * in.tint.rgb) * a * in.emission, a);
}

fn median3(c: vec3<f32>) -> f32 {
    return max(min(c.r, c.g), min(max(c.r, c.g), c.b));
}

// An MSDF glyph. effect_params: x = mode (0 fill with an optional outline,
// 1 a soft layer for shadows and glow), y = outline width or soft spread
// (base pixels), z = softness (base pixels), w = distance range (base
// pixels). effect_extra = outline colour.
@fragment
fn fs_text_msdf(in: VertexOutput) -> @location(0) vec4<f32> {
    let range = max(in.effect_params.w, 1e-4);
    let d = median3(textureSample(sprite_texture, sprite_sampler, in.uv).rgb) - 0.5;
    let texels = vec2<f32>(textureDimensions(sprite_texture, 0));
    let unit = vec2<f32>(range) / texels;
    let screen = vec2<f32>(1.0) / max(fwidth(in.uv), vec2<f32>(1e-6));
    let px_range = max(0.5 * dot(unit, screen), 1.0);
    // Signed distance in screen pixels, positive inside
    let dist = d * px_range;
    let to_screen = px_range / range;
    if in.effect_params.x < 0.5 {
        let fill = clamp(dist + 0.5, 0.0, 1.0) * in.tint.a;
        let width = in.effect_params.y * to_screen;
        let fill_rgb = srgb_to_linear(in.tint.rgb);
        if width <= 0.0 {
            return vec4<f32>(fill_rgb * fill * in.emission, fill);
        }
        let outer = clamp(dist + width + 0.5, 0.0, 1.0) * in.effect_extra.a;
        let rgb = fill_rgb * fill + srgb_to_linear(in.effect_extra.rgb) * outer * (1.0 - fill);
        return vec4<f32>(rgb * in.emission, fill + outer * (1.0 - fill));
    }
    let spread = in.effect_params.y * to_screen;
    let soft = max(in.effect_params.z * to_screen, 1.0);
    let a = smoothstep(-spread - soft, -spread + soft, dist) * in.tint.a;
    return vec4<f32>(srgb_to_linear(in.tint.rgb) * a, a);
}

// A UI rectangle: anti-aliased rounded corners and an optional border.
// effect_params: x = corner radius, y = border width, zw = size (UI units).
// effect_extra = border colour.
@fragment
fn fs_ui_rect(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = max(in.effect_params.zw, vec2<f32>(1e-4));
    let p = (in.uv - vec2<f32>(0.5)) * size;
    let r = clamp(in.effect_params.x, 0.0, 0.5 * min(size.x, size.y));
    let q = abs(p) - 0.5 * size + vec2<f32>(r);
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let aa = max(length(fwidth(p)) * 0.70710678, 1e-4);
    let outer = clamp(0.5 - d / aa, 0.0, 1.0);
    let fill_rgb = srgb_to_linear(in.tint.rgb);
    let border = in.effect_params.y;
    if border <= 0.0 {
        let a = outer * in.tint.a;
        return vec4<f32>(fill_rgb * a, a);
    }
    let inner = clamp(0.5 - (d + border) / aa, 0.0, 1.0);
    let a_fill = inner * in.tint.a;
    let a_border = (outer - inner) * in.effect_extra.a;
    let rgb = fill_rgb * a_fill + srgb_to_linear(in.effect_extra.rgb) * a_border;
    return vec4<f32>(rgb, a_fill + a_border);
}
```

Notes:
- `length(fwidth(p)) * 0.7071` is one screen pixel in UI units on an axis-aligned rect. An integer-aligned rect then covers its pixels exactly (centre at d = -0.5 gives coverage 1), as the old CPU rect did.
- Check the `VertexOutput` field names (`uv`, `tint`, `effect_params`, `effect_extra`, `emission`) against `sprite.wgsl:38-46`. Rename here if they differ.
- `in.emission` is the instance's emission lane (1 by default).

**The UI's gamma output.** In `src/core/shaders/include/color.wgsl`, add:

```wgsl
// Set (pipeline override) for pipelines drawing the UI image: its texels
// are gamma-encoded premultiplied colour, so blending happens in sRGB
// space as on the web
override UI_GAMMA: bool = false;

// A fragment's premultiplied linear colour, re-encoded to premultiplied
// sRGB for the UI image
fn ui_out(c: vec4<f32>) -> vec4<f32> {
    if !UI_GAMMA {
        return c;
    }
    if c.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(linear_to_srgb(clamp(c.rgb / c.a, vec3<f32>(0.0), vec3<f32>(1.0))) * c.a, c.a);
}
```

Then wrap the returned value of every fragment entry in `ui_out(..)`:
- `core/sprite.wgsl` (`fs_base*`, `fs_effect*`, and the custom-shader entries if they return from that file);
- `core/line.wgsl` (`fs_main`, `fs_textured`);
- the four entries in `text.wgsl` above.

When `UI_GAMMA` is false (every world pipeline), `ui_out` returns its input, and the compiler folds it away.

In `pipelines.rs` `batch_pipeline`, set the fragment state's `compilation_options` constants to `[("UI_GAMMA", 1.0)]` when `key.format == UI_FORMAT`. Leave them empty otherwise. In wgpu 30 this is `wgpu::PipelineCompilationOptions { constants: &[("UI_GAMMA", 1.0)], ..Default::default() }`; if it takes a `HashMap`, build one. Custom game shaders compose `color.wgsl` too. Make sure their composition does not declare `UI_GAMMA` twice; it is declared only in `color.wgsl`.

The pipeline check in Step 1 builds every new shader in `UI_FORMAT`. Extend it to build `BatchShader::Sprite` and `BatchShader::Line` (stream `Triangles`) in `UI_FORMAT` too, so the override is exercised on every module the UI uses.

In `compose.rs`, append `core/text.wgsl` to `SPRITE`'s `concat!`, after `core/sprite.wgsl`. `fs_ui_rect` uses no group 1 binding, so a pipeline built with the `camera_only` layout validates. (wgpu checks only the bindings an entry point uses.)

- [ ] **Step 5: Pipelines, placing and drawing**

In `key.rs`, add `TextMask`, `TextColor`, `TextMsdf` and `UiRect` to `BatchShader`, each with a one-line doc. In `pipelines.rs` `ensure`, add these rows before the catch-all:

```rust
      (BatchShader::TextMask, Stream::Quads, _, _) => (&self.sprite, &self.layout, "fs_text_mask"),
      (BatchShader::TextColor, Stream::Quads, _, _) => (&self.sprite, &self.layout, "fs_text_color"),
      (BatchShader::TextMsdf, Stream::Quads, _, _) => (&self.sprite, &self.layout, "fs_text_msdf"),
      (BatchShader::UiRect, Stream::Quads, _, _) => (&self.sprite, &self.camera_only, "fs_ui_rect"),
```

(Match the tuple shape `ensure` uses: the summary shows `(shader, stream, premultiplied, linear)`.)

In `gpu.rs`:
- **Per-camera format:** add `format: wgpu::TextureFormat` to `CameraDraws`, set in `prepare` as `if camera.ui { UI_FORMAT } else { self.format }`. Pass that format to every `pipelines.ensure` in that camera's `place` closure. `draw` looks up its camera's `CameraDraws` for the format (`self.draws.iter().find(|d| d.camera == Some(camera))`, falling back to `self.format`).
- **`place` arms:**
  - `DrawPayload::Glyph(i)`: ensure the glyph's shader pipeline. Push `glyph.instance` as is; it already carries its atlas UVs, so there is no `place_in_slot`. Return `Placed::Quad(DrawState { page: Some(SlotPage::Glyph(glyph.page)), shader: glyph.shader, blend: glyph.blend, sampling: glyph.sampling, scissor: glyph.scissor })`.
  - `DrawPayload::Shape(i)`: ensure the `UiRect` pipeline, push `shape.instance`, and return `Placed::Quad(DrawState { page: None, shader: BatchShader::UiRect, blend: shape.blend, sampling: Sampling::Linear, scissor: shape.scissor })`.
  - `Sprite` and `Triangles`: copy `scissor` from their payloads into the `DrawState` instead of `None`.
- **`draw`:**
  - Add the `image: (u32, u32)` parameter.
  - Let the untextured-quad guard pass `BatchShader::UiRect` as well as lights.
  - Before drawing:

```rust
    match draw.state.scissor {
      Some(rect) => {
        let x = rect.x.min(image.0);
        let y = rect.y.min(image.1);
        let width = rect.width.min(image.0 - x);
        let height = rect.height.min(image.1 - y);
        if width == 0 || height == 0 {
          return;
        }
        pass.set_scissor_rect(x, y, width, height);
      }
      None => pass.set_scissor_rect(0, 0, image.0.max(1), image.1.max(1)),
    }
```

(A scissor past the attachment is a wgpu validation error; this clamps it.)

In `context.rs`, add `pub(crate) fn camera_draw(&self) -> Option<&CameraDraw>` (the camera the pass runs for). In `passes/world.rs` `replay`, pass `ctx.camera_draw().map_or(ctx.render_size(), |c| c.rect.size())` as `image`. Read it before taking `encoder_and_current_view`, to avoid a double borrow. Fix every other `batch.draw(..)` call site the same way (`grep -rn "\.draw(&mut pass" src/core/render`); the lighting pass's light draws, if they go through `draw`, use that camera's light-target size.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p ryo2d --lib draw_list plan:: the_text_and_ui_pipelines_build`
Expected: pass.
Run: `cargo test --test e2e`
Expected: everything that passed before still passes. The scissor reset to the full image must not change any picture.

- [ ] **Step 7: Docs**

In `src/core/render/AGENTS.md` "The 2D Batch" add:
- **Glyph and shape items.**
  - Glyphs: `DrawPayload::Glyph`, already placed in their glyph page, drawn by `TextMask`, `TextColor` or `TextMsdf`.
  - UI rects: `DrawPayload::Shape`, untextured, drawn by `UiRect` with an analytic rounded rect and border.
  - The lane table above.
- **Scissor.** `DrawState::scissor` is applied per draw and clamped to the image; `None` is the whole image.
- **Per-camera format.** World cameras draw in `SCENE_FORMAT`. A UI camera (`CameraDraw::ui`) draws in `UI_FORMAT` (`Rgba8Unorm`, gamma-encoded premultiplied, through the `UI_GAMMA` override and `ui_out`) and sees only items drawn for it.

In "Draw Order", add that `Text` sorts after entities at equal z.

- [ ] **Step 8: Gate and commit**

```bash
git add -A
git commit -m "feat: glyph and UI rect draws in the 2D batch" -m "- coverage, colour and MSDF text shaders with outline, shadow and glow lanes
- anti-aliased rounded UI rects with borders
- scissor rectangles applied per draw; cameras draw in their own colour format"
```

### Task 7: World text (`Text2d`)

**Files:**
- Create: `src/core/text/world.rs`, `src/core/render/text.rs`, `tests/e2e/text.rs`
- Modify:
  - `src/core/text/mod.rs`
  - `src/core/render/mod.rs` (`mod text;`)
  - `src/core/render/draw_list.rs` (`set_last_sprite_scissor`)
  - `src/core/engine/mod.rs`:
    - the `text: TextEngine` and `glyph_raster: GlyphRasterizer` fields;
    - register `advance_text_reveal`;
    - register the `TextRevealed` message.
  - `src/core/engine/extract.rs` (world text in `present_frame`, prewarm, `text.end_frame()`)
  - `src/core/messages/mod.rs` (register `TextRevealed`)
  - `src/lib.rs` (prelude: `Text2d`, `TextBounds`, `TextReveal`, `TextRevealed`)
  - `tests/e2e.rs` (mount `e2e/text.rs`)
  - `src/core/testing/gpu_world.rs` (the steady-frame check gains a `Text2d`)
  - `src/core/text/AGENTS.md`, `src/core/render/AGENTS.md`

**Interfaces:**
- Consumes:
  - Tasks 2-6: `TextEngine`, `TextLayout`, `GlyphStyle`, `TextEffects`, `TextIcons`, `reveal_alpha`, `RevealClock`, `GlyphRasterizer`, `GlyphKey`, `raster_size_step`, `glyph_id`;
  - `SpriteGpuRenderer::glyph`, `DrawList::push_glyph`, `DrawGlyph`, `DrawKind::Text`, `BatchShader::Text*`;
  - `render_global` (`ecs/interpolate.rs`), `CameraDraw`, `CameraView::{scale, rotation, rect_min, screen_to_world, is_visible}`.
- Produces:
  - **Public:**
    - `Text2d { text: RichText, style: TextStyle, layout: TextLayoutOptions, lit: bool, z_order: i32, blend: BlendMode }` (`#[non_exhaustive]`, `#[require(Transform, TextBounds)]`):
      - constructors `new(text)` and `markup(&str)`, with anchor `Anchor::Center`;
      - builders `with_style`, `with_layout`, `with_lighting(bool)`, `with_z(i32)`, `with_blend`;
      - `set_text`;
    - `TextBounds { size: Vec2, offset: Vec2 }`, where `offset` runs from the entity's position to the block's top-left before scale and rotation;
    - `TextReveal`: `new(per_second)`, `with_punctuation_pause`, `with_fade`, `restart`, `is_done`, `shown`;
    - `#[derive(Message)] TextRevealed { entity: Entity }`.
  - **Crate-internal:**
    - `TextLayoutState { layout: Arc<TextLayout>, key: u64 }` (component);
    - `render::text::{TextSources, TextTargets, TextPlacement, TextDraw, emit_text, layout_world_text, emit_world_text, prewarm}`;
    - `TextEngine::generation() -> u64`.

**Placement math** (used by world text here and by the UI in Task 8):

| Term | Meaning |
|---|---|
| Layout pixels | World units for world text (layout `scale` 1); render pixels for the UI (`scale` = the UI render scale) |
| `TextPlacement::origin` | The block's top-left in target units |
| `axis_x`, `axis_y` | Target units per layout pixel along the block's axes (rotation and scale folded in) |
| `px_per_layout` | Render pixels per layout pixel: the raster size factor |
| `k` | Layout pixels per image pixel: `g.size / raster_px` for raster and pixel glyphs, `g.size / base` for MSDF |

How a glyph is placed:
- **Image rectangle.** A glyph image `(left, top, w, h)` drawn at pen `p` (layout pixels) covers layout pixels from `p + (left·k, -top·k)` to that plus `(w·k, h·k)`.
- **Pixel grid.** When the placement is axis-aligned (no rotation, positive scale), glyphs land on whole render pixels through `pixel_grid = (target point of render pixel (0, 0), render pixels per target unit)`:
  1. The pen's render-pixel x splits into `floor` and fraction. The fraction picks the subpixel bin. A bin that rounds up to 4 carries into the next whole pixel.
  2. The baseline is rounded.
  3. The image maps 1:1 onto render pixels.
- **Sizes.** Raster sizes are exact in the UI. World text snaps them to `raster_size_step`, so a zooming camera reuses glyphs.

- [ ] **Step 1: Write the failing tests**

Create `tests/e2e/text.rs` and mount it in `tests/e2e.rs` (`#[path = "e2e/text.rs"] mod text;`):

```rust
//! World text and text rendering, pixel-checked

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// A scene that spawns what `setup` asks and leaves it
struct Spawn<F: FnMut(&mut SceneContext) + 'static>(F);

impl<F: FnMut(&mut SceneContext) + 'static> Scene for Spawn<F> {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    (self.0)(ctx);
  }
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn frame_of(size: (u32, u32), builder: impl FnOnce(EngineBuilder) -> EngineBuilder, setup: impl FnMut(&mut SceneContext) + 'static) -> Option<image::RgbaImage> {
  let base = EngineBuilder::new().resolution(size.0, size.1).start_scene(Spawn(setup));
  let mut engine = HeadlessEngine::new_or_skip(builder(base))?;
  engine.step_frames(3).unwrap();
  Some(engine.frame().unwrap())
}

fn bright(p: &image::Rgba<u8>) -> bool {
  p.0[0] > 200 && p.0[1] > 200 && p.0[2] > 200
}

fn count(frame: &image::RgbaImage, f: impl Fn(&image::Rgba<u8>) -> bool) -> usize {
  frame.pixels().filter(|p| f(p)).count()
}

#[test]
fn world_text_draws_where_its_entity_is() {
  let Some(frame) = frame_of((96, 64), |b| b, |ctx| {
    ctx.camera().set_position(0.0, 0.0);
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), Text2d::new("HH").with_style(TextStyle::new().size(24.0))));
  }) else { return };
  let (w, h) = frame.dimensions();
  let centre = (w / 2 - 12..w / 2 + 12).flat_map(|x| (h / 2 - 8..h / 2 + 8).map(move |y| (x, y)));
  assert!(centre.filter(|&(x, y)| bright(frame.get_pixel(x, y))).count() > 20, "text at the centre");
  assert!(!bright(frame.get_pixel(2, 2)), "nothing in the corner");
}

#[test]
fn a_missing_glyph_draws_a_box() {
  let Some(frame) = frame_of((64, 64), |b| b, |ctx| {
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), Text2d::new("\u{6F22}").with_style(TextStyle::new().size(32.0))));
  }) else { return };
  assert!(count(&frame, bright) > 10, "the tofu box is drawn");
}

#[test]
fn non_ascii_glyphs_draw() {
  let Some(frame) = frame_of((96, 64), |b| b, |ctx| {
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), Text2d::new("\u{03A9}\u{0436}\u{00E9}").with_style(TextStyle::new().size(28.0))));
  }) else { return };
  assert!(count(&frame, bright) > 30, "Greek, Cyrillic and Latin-1 glyphs draw");
}

#[test]
fn zoomed_world_text_stays_sharp() {
  let Some(frame) = frame_of((128, 96), |b| b, |ctx| {
    ctx.camera().set_zoom(4.0);
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), Text2d::new("Il").with_style(TextStyle::new().size(10.0))));
  }) else { return };
  let lit = count(&frame, |p| p.0[0] > 24);
  let solid = count(&frame, |p| p.0[0] > 230);
  assert!(lit > 50);
  assert!(solid * 10 >= lit * 6, "mostly solid, not an upscaled blur: {solid} of {lit}");
}

#[test]
fn an_msdf_outline_surrounds_the_fill() {
  let Some(frame) = frame_of((96, 64), |b| b, |ctx| {
    let mut fonts = ctx.fonts();
    let ui = fonts.role(FontRole::Ui).unwrap();
    fonts.set_render(ui, FontRender::msdf());
    drop(fonts);
    let style = TextStyle::new().size(40.0).color(Color::rgb(255, 0, 0)).outline(1.5, Color::WHITE);
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), Text2d::new("O").with_style(style)));
  }) else { return };
  assert!(count(&frame, |p| p.0[0] > 200 && p.0[1] < 60) > 20, "red fill");
  assert!(count(&frame, bright) > 20, "white outline");
}

#[test]
fn msdf_glow_reaches_past_the_glyph() {
  let glowing = |glow: bool| frame_of((96, 64), |b| b, move |ctx| {
    let mut fonts = ctx.fonts();
    let ui = fonts.role(FontRole::Ui).unwrap();
    fonts.set_render(ui, FontRender::msdf());
    drop(fonts);
    let mut style = TextStyle::new().size(40.0).color(Color::rgb(0, 0, 255));
    if glow {
      style = style.glow(1.5, Color::rgb(0, 255, 0));
    }
    ctx.world.spawn((Transform::from_xy(0.0, 0.0), Text2d::new("I").with_style(style)));
  });
  let (Some(plain), Some(glow)) = (glowing(false), glowing(true)) else { return };
  assert_eq!(count(&plain, |p| p.0[1] > 40), 0);
  assert!(count(&glow, |p| p.0[1] > 40) > 10, "green glow around the blue glyph");
}

#[test]
fn a_pixel_font_lands_on_whole_pixels() {
  let Some(frame) = frame_of((160, 90), |b| b.pixel_art(160, 90), |ctx| {
    let style = TextStyle::new().role(FontRole::Pixel).size(16.0).color(Color::WHITE);
    ctx.world.spawn((Transform::from_xy(0.3, 0.6), Text2d::new("HELLO").with_style(style)));
  }) else { return };
  let mid = count(&frame, |p| p.0[0] > 10 && p.0[0] < 245);
  assert_eq!(mid, 0, "every pixel is background or text colour");
  assert!(count(&frame, |p| p.0[0] >= 245) > 20);
}

#[test]
fn the_reveal_shows_text_over_time_and_says_so_once() {
  struct Reveal {
    entity: Option<Entity>,
    revealed: usize,
  }
  impl Scene for Reveal {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      ctx.look().tonemap = Tonemap::Clamp;
      let entity = ctx
        .world
        .spawn((Transform::from_xy(0.0, 0.0), Text2d::new("HHHHHH").with_style(TextStyle::new().size(20.0)), TextReveal::new(6.0)))
        .id();
      self.entity = Some(entity);
    }
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      self.revealed += ctx.messages::<TextRevealed>().iter().filter(|m| Some(m.entity) == self.entity).count();
      SceneAction::Continue
    }
  }
  let builder = EngineBuilder::new().resolution(128, 64).fixed_hz(60).start_scene(Reveal { entity: None, revealed: 0 });
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else { return };
  engine.step_frames(20).unwrap();
  let early = count(&engine.frame().unwrap(), bright);
  engine.step_frames(100).unwrap();
  let late = count(&engine.frame().unwrap(), bright);
  assert!(late > early * 2, "more shows later: {early} then {late}");
}

#[test]
fn a_text_entity_scaled_to_zero_draws_nothing() {
  let Some(frame) = frame_of((64, 64), |b| b, |ctx| {
    ctx.world.spawn((Transform::from_xy(0.0, 0.0).with_scale(Vec2::ZERO), Text2d::new("HHH").with_style(TextStyle::new().size(24.0))));
  }) else { return };
  assert_eq!(count(&frame, bright), 0);
}
```

Adapt these names to the current APIs while writing:
- `ctx.messages::<T>()`: check the `ryo-messages` skill for the scene-side reader.
- `EngineBuilder::fixed_hz` and `Transform::with_scale`: check the prelude.
- The reveal test counts messages in `update`. Assert `revealed == 1` at the end; to read it, hold the count in an `Arc<AtomicUsize>` owned by the test.

Add two library tests in `src/core/text/world.rs` (with `test_util` worlds, no GPU), driving `layout_world_text` directly:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::ecs::{Transform, World};
  use crate::core::text::{FontRole, Fonts, TextEngine};

  #[test]
  fn text_bounds_report_the_block() {
    let mut world = World::new();
    world.insert_resource(Fonts::with_builtins());
    let mut engine = TextEngine::new();
    let e = world.spawn((Transform::default(), Text2d::new("Hello").with_style(crate::core::text::TextStyle::new().size(20.0)))).id();
    crate::core::render::text::layout_world_text(&mut world, &mut engine);
    let bounds = world.get::<TextBounds>(e).unwrap();
    assert!(bounds.size.x > 30.0 && bounds.size.y > 20.0);
    assert!((bounds.offset + bounds.size / 2.0).length() < 0.01, "centred by default");
  }

  #[test]
  fn world_text_relays_out_when_its_font_arrives() {
    let mut world = World::new();
    world.insert_resource(Fonts::with_builtins());
    let mut engine = TextEngine::new();
    let e = world.spawn((Transform::default(), Text2d::new("iiii"))).id();
    crate::core::render::text::layout_world_text(&mut world, &mut engine);
    let before = world.get::<TextBounds>(e).unwrap().size.x;
    let mono = world.resource::<Fonts>().role(FontRole::Mono).unwrap();
    world.resource_mut::<Fonts>().set_role(FontRole::Ui, mono);
    crate::core::render::text::layout_world_text(&mut world, &mut engine);
    let after = world.get::<TextBounds>(e).unwrap().size.x;
    assert!(after > before + 5.0, "monospace i is wider: {before} then {after}");
  }
}
```

(`layout_world_text` syncs the engine with `Fonts` itself.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test e2e text::` and `cargo test -p ryo2d --lib text::world`
Expected: compile errors (`Text2d` not found).

- [ ] **Step 3: Write `text/world.rs`**

```rust
//! Text on entities: `Text2d`, its laid-out bounds, and the typewriter reveal

use std::sync::Arc;

use glam::Vec2;

use crate::core::ecs::{Component, Entity, Message, Transform};
use crate::core::render::batch::BlendMode;
use crate::core::text::engine::RevealClock;
use crate::core::text::{RichText, TextLayout, TextLayoutOptions, TextStyle};
use crate::core::ui::Anchor;

/// Text drawn in the world, sorted with sprites by `z_order`. Sizes and
/// wrap widths are world units; the entity's transform moves, turns and
/// scales it. Unlit by default, so labels stay readable in the dark.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(Transform, TextBounds)]
#[non_exhaustive]
pub struct Text2d {
  pub text: RichText,
  pub style: TextStyle,
  pub layout: TextLayoutOptions,
  pub lit: bool,
  pub z_order: i32,
  pub blend: BlendMode,
}

impl Text2d {
  /// Plain or rich text, centred on the entity
  pub fn new(text: impl Into<RichText>) -> Self {
    Self {
      text: text.into(),
      style: TextStyle::default(),
      layout: TextLayoutOptions::default().anchor(Anchor::Center),
      lit: false,
      z_order: 0,
      blend: BlendMode::Alpha,
    }
  }

  /// Text from markup
  pub fn markup(source: &str) -> Self {
    Self::new(RichText::markup(source))
  }

  pub fn with_style(mut self, style: TextStyle) -> Self {
    self.style = style;
    self
  }

  pub fn with_layout(mut self, layout: TextLayoutOptions) -> Self {
    self.layout = layout;
    self
  }

  /// Lit text darkens and colours with the scene's lights, like sprites
  pub fn with_lighting(mut self, lit: bool) -> Self {
    self.lit = lit;
    self
  }

  pub fn with_z(mut self, z_order: i32) -> Self {
    self.z_order = z_order;
    self
  }

  pub fn with_blend(mut self, blend: BlendMode) -> Self {
    self.blend = blend;
    self
  }

  /// Replace the text (a reveal on the entity starts over)
  pub fn set_text(&mut self, text: impl Into<RichText>) {
    self.text = text.into();
  }
}

/// The laid-out size of a `Text2d`, in world units before the entity's
/// scale; `offset` is the block's top-left relative to the position
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct TextBounds {
  pub size: Vec2,
  pub offset: Vec2,
}

/// Reveal a `Text2d` a character cluster at a time, on game time
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TextReveal {
  /// Clusters per second
  pub per_second: f32,
  /// Seconds to hold after `. , ! ? ; :`
  pub punctuation_pause: f32,
  /// Seconds each character takes to fade in (0: appears at once)
  pub fade: f32,
  pub(crate) clock: RevealClock,
}

impl TextReveal {
  pub fn new(per_second: f32) -> Self {
    Self { per_second, punctuation_pause: 0.0, fade: 0.0, clock: RevealClock::default() }
  }
  pub fn with_punctuation_pause(mut self, seconds: f32) -> Self {
    self.punctuation_pause = seconds;
    self
  }
  pub fn with_fade(mut self, seconds: f32) -> Self {
    self.fade = seconds;
    self
  }
  /// Start the reveal over
  pub fn restart(&mut self) {
    self.clock = RevealClock::default();
  }
  pub fn is_done(&self) -> bool {
    self.clock.done
  }
  /// Character clusters shown so far (fractional while fading)
  pub fn shown(&self) -> f32 {
    self.clock.shown
  }
}

/// A `TextReveal` finished showing its text
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRevealed {
  pub entity: Entity,
}

/// A `Text2d`'s current layout and the hash it was made for
#[derive(Component, Debug, Clone)]
pub(crate) struct TextLayoutState {
  pub(crate) layout: Arc<TextLayout>,
  pub(crate) key: u64,
}
```

Add the system that advances reveals. It runs in `PostUpdateSet::Extract` on game time and reads the layout made last frame:

```rust
/// Advance every `TextReveal` by the game delta; write `TextRevealed` once
pub(crate) fn advance_text_reveal(
  time: crate::core::ecs::Res<crate::core::time::Time>,
  mut reveals: crate::core::ecs::Query<(Entity, &mut TextReveal, &TextLayoutState)>,
  mut finished: crate::core::ecs::MessageWriter<TextRevealed>,
) {
  let dt = time.game.delta();
  for (entity, mut reveal, state) in &mut reveals {
    let (per_second, pause) = (reveal.per_second, reveal.punctuation_pause);
    if reveal.clock.step(dt, per_second, pause, &state.layout) {
      finished.write(TextRevealed { entity });
    }
  }
}
```

Adjust `time.game.delta()` and the `Res`/`Query` paths to `time.rs` and the kernel re-exports. Register the system where `advance_parallax` is registered (grep for it). Register `TextRevealed` in `register_engine_messages`.

Mounting the module: `mod world; pub use world::{Text2d, TextBounds, TextReveal, TextRevealed}; pub(crate) use world::{TextLayoutState, advance_text_reveal};`. Also make `engine::RevealClock` reachable as `crate::core::text::engine::RevealClock`, with `engine` `pub(crate) mod`.

- [ ] **Step 4: Write `render/text.rs` (the emitter)**

```rust
//! Glyph quads from laid-out text, for world text (one set per camera) and
//! the UI (the UI camera). Three passes per block: shadows, outlines, fills,
//! so one glyph's outline never covers its neighbour's fill.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use glam::Vec2;

use crate::core::ecs::{Entity, GlobalTransform, PreviousGlobalTransform, RenderLayers, World};
use crate::core::graphics::{SpriteGpuRenderer, SpriteInstance};
use crate::core::render::batch::{BatchShader, BlendMode, Filter, Sampling};
use crate::core::render::{CameraDraw, DrawGlyph, DrawKind, DrawList, PixelRect, SpriteLook, SpriteShading};
use crate::core::text::{
  FaceSource, FontRender, Fonts, GlyphContext, GlyphFormat, GlyphKey, GlyphMod, GlyphRasterizer,
  Icon, PlacedGlyph, PlacedIcon, Text2d, TextBounds, TextEffects, TextEngine, TextIcons, TextLayout,
  TextLayoutState, TextReveal, glyph_id, raster_size_step, reveal_alpha,
};
use crate::core::ui::{Anchor, Color};

/// What the text reads
pub(crate) struct TextSources<'a> {
  pub(crate) fonts: &'a Fonts,
  pub(crate) engine: &'a TextEngine,
  pub(crate) effects: &'a TextEffects,
  pub(crate) icons: &'a TextIcons,
}

/// What the text writes into
pub(crate) struct TextTargets<'a> {
  pub(crate) raster: &'a mut GlyphRasterizer,
  pub(crate) sprites: &'a mut SpriteGpuRenderer,
  pub(crate) device: &'a wgpu::Device,
  pub(crate) queue: &'a wgpu::Queue,
  pub(crate) list: &'a mut DrawList,
  /// Warn-once keys (glow on a raster font, unknown icons, zero scale)
  pub(crate) warned: &'a mut HashSet<u64>,
}

/// The draw-list side of a block
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextDraw {
  pub(crate) kind: DrawKind,
  pub(crate) z: i32,
  pub(crate) lit: bool,
  pub(crate) only_camera: Option<Entity>,
  pub(crate) layers: RenderLayers,
  pub(crate) scissor: Option<PixelRect>,
  pub(crate) blend: BlendMode,
}

/// Where and how a block draws (see the placement math in the plan)
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextPlacement {
  pub(crate) origin: Vec2,
  pub(crate) axis_x: Vec2,
  pub(crate) axis_y: Vec2,
  pub(crate) px_per_layout: f32,
  pub(crate) size_steps: bool,
  /// Target point of render pixel (0, 0) and render pixels per target unit
  pub(crate) pixel_grid: Option<(Vec2, f32)>,
  pub(crate) draw: TextDraw,
  pub(crate) time: f32,
  /// Clusters shown and the fade length in clusters
  pub(crate) reveal: Option<(f32, f32)>,
  pub(crate) seed: u64,
  pub(crate) opacity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
  Shadow,
  Outline,
  Fill,
}

/// Lay a block's glyphs and icons into the draw list
pub(crate) fn emit_text(sources: &TextSources<'_>, targets: &mut TextTargets<'_>, layout: &TextLayout, place: &TextPlacement) {
  if !(place.px_per_layout.is_finite() && place.px_per_layout > 0.0) {
    return;
  }
  for layer in [Layer::Shadow, Layer::Outline, Layer::Fill] {
    for (index, glyph) in layout.glyphs.iter().enumerate() {
      emit_glyph(sources, targets, layout, place, index, glyph, layer);
    }
  }
  for icon in &layout.icons {
    emit_icon(sources, targets, place, icon);
  }
}
```

Then write the glyph and icon functions. Each step below says exactly what to compute.

- **`fn modifier(..) -> Option<(GlyphMod, f32)>`** returns the effect's `GlyphMod` and the glyph's alpha, or `None` when the glyph is invisible:
  - Alpha is `place.opacity`, times `reveal_alpha(glyph.cluster, shown, fade)` when `place.reveal` is set, times `GlyphMod::alpha`.
  - The `GlyphMod` comes from `sources.effects.apply(effect, &GlyphContext { index, cluster, line: 0, time: place.time, size: glyph.size / layout.scale, seed: place.seed, args: String::new() })` when the glyph's style has an effect; otherwise it is `GlyphMod::default()`.
  - Construct `GlyphContext` through a `pub(crate) fn new(..)` on it, since it is `#[non_exhaustive]`.
- **`emit_glyph`:**
  1. **Look and pen.** Let `style = &layout.styles[glyph.style as usize]`, `color = modifier.color.unwrap_or(style.color)`, and `pen = glyph.pos + modifier.offset * layout.scale` (layout pixels). For a shadow, add `shadow.offset * layout.scale` to the pen.
  2. **Branch on the face** (`sources.fonts.face(glyph.font)`) and its render mode (`sources.engine.face_render(glyph.font)`):
     - **Bitmap face** (`FaceSource::Bitmap`), fill layer only (shadow layer: the same quad tinted with the shadow colour at the shadow offset):
       - The cell `i = glyph.glyph` sits at `col = i % columns`, `row = i / columns`. Its UV is `(col·cw/W, row·ch/H, (col+1)·cw/W, (row+1)·ch/H)`, where `W` and `H` are the sheet size (`sprite.width()`, `height()`).
       - `k = glyph.size / ch`. The cell covers layout pixels `pen + (0, -ch·k)` to `+ (cw·k, ch·k)`.
       - Push it as a sprite: `texture = targets.sprites.texture_of(&bitmap.sprite)`, `SpriteLook { shading: SpriteShading::Plain, blend: place.draw.blend, filter: Filter::Nearest, lit }`. Then call `list.set_last_sprite_scissor(place.draw.scissor)`.
     - **`FontRender::Raster`:**
       - The raster pixel size is `r = glyph.size * place.px_per_layout`, snapped with `raster_size_step` when `place.size_steps`; skip the glyph if `r` is not finite or is `<= 0`.
       - Per layer:
         - **Fill:** `GlyphKey::fill(font, glyph, r, fraction)`, with `coords`, `skew` and `embolden` copied from the glyph.
         - **Outline** (if `style.outline`): `GlyphKey::stroke(.., width_px)`, with `width_px = 2 * outline.width * r * layout.scale / glyph.size`. The band is centred, so twice the width puts `width` outside the glyph. Tint it with the outline colour.
         - **Shadow** (if `style.shadow`): the fill key at the shadow's pen, tinted with the shadow colour (hard).
         - **Glow:** warned once per style hash with `engine_warn!(module: "text", "glow needs an MSDF font (FontRender::Msdf); drawn without it")` and skipped.
       - Sampling is `Linear`.
     - **`FontRender::Pixel { design_size }`:**
       - `n = (glyph.size * px_per_layout / design_size).round().max(1.0)` and `r = design_size * n`, with `GlyphKey::pixel`.
       - The outline is four copies offset by `±round(outline.width * px_per_layout)` render pixels on x and y, tinted with the outline colour, drawn in the outline layer. The shadow is a hard copy.
       - Sampling is `Nearest`.
     - **`FontRender::Msdf { base, range }`:**
       - One key, `GlyphKey::msdf(font, glyph, base, range)`, with `k = glyph.size / base` and no snapping.
       - Let `base_per_unit = base * layout.scale / glyph.size` (base pixels per text unit). The layers:
         - **Shadow:** mode 1, spread 0, softness `shadow.softness * base_per_unit`, at the shadow offset.
         - **Glow:** in the shadow layer, before the shadow; mode 1, spread `glow.radius * base_per_unit`, softness `glow.radius * base_per_unit * 0.5`, tinted with the glow colour.
         - **Fill:** mode 0, width `outline.width * base_per_unit` (0 without an outline), `effect_extra` the outline colour.
       - In all of them, `effect_params.w = range`.
       - Sampling is `Linear`.
  3. **Fetch the slot.** For each key the branch makes, `targets.sprites.glyph(device, queue, key, || targets.raster.rasterize(sources.fonts, coords, &key))`, with `coords = sources.engine.coords(glyph.coords)`. `None` (a space, a refused glyph) skips that quad. Note the closure borrows `targets.raster` while `targets.sprites` is mutably borrowed: destructure `TextTargets` into locals first.
  4. **Place the quad** with the helper below, then build the instance:
     - `SpriteInstance::new().with_transform(centre.x, centre.y, rotation, size.x, size.y).with_uv(slot.uv[0], slot.uv[1], slot.uv[2], slot.uv[3]).with_tint(color, alpha)`, then `uv_bounds = slot.uv` and the MSDF lanes.
     - `GlyphMod::scale` multiplies `size`, about the centre.
     - `shader` comes from `slot.page.format`: `Mask` is `TextMask`, `Color` is `TextColor`, `Msdf` is `TextMsdf`.
     - `min` and `max` are `centre ± Vec2::splat(size.length() / 2.0)`.
     - Push with `list.push_glyph(place.draw.kind, place.draw.z, place.draw.lit, place.draw.only_camera, DrawGlyph { .. })`.

```rust
/// Centre, size (target units) and rotation of a glyph image's quad
fn quad(place: &TextPlacement, pen: Vec2, slot_left: i32, slot_top: i32, w: u32, h: u32, k: f32, snap: bool) -> (Vec2, Vec2, f32, f32) {
  // Returns (centre, size, rotation, subpixel fraction for the key)
  if snap && let Some((grid, ppu)) = place.pixel_grid {
    let pen_t = place.origin + place.axis_x * pen.x + place.axis_y * pen.y;
    let px = (pen_t - grid) * ppu;
    let mut ix = px.x.floor();
    let fraction = px.x - ix;
    if (fraction * 4.0).round() >= 4.0 {
      ix += 1.0;
    }
    let iy = px.y.round();
    let top_left = Vec2::new(ix + slot_left as f32, iy - slot_top as f32);
    let size = Vec2::new(w as f32, h as f32);
    let centre = grid + (top_left + size / 2.0) / ppu;
    return (centre, size / ppu, 0.0, fraction);
  }
  let top_left = pen + Vec2::new(slot_left as f32 * k, -(slot_top as f32) * k);
  let local = top_left + Vec2::new(w as f32 * k, h as f32 * k) / 2.0;
  let centre = place.origin + place.axis_x * local.x + place.axis_y * local.y;
  let size = Vec2::new(w as f32 * k * place.axis_x.length(), h as f32 * k * place.axis_y.length());
  (centre, size, place.axis_x.y.atan2(place.axis_x.x), 0.0)
}
```

The subpixel fraction is needed before the key exists, and the slot's placement only after. Compute the fraction first from the pen alone: `fraction = ((pen_t - grid) * ppu).x` minus its floor. Build the key, fetch the slot, then call `quad`. Raster fills and strokes snap, as do pixel glyphs; MSDF never does.

- **`emit_icon`:**
  - Resolve the icon. `Icon::Sprite { sprite, .. }` is the sprite, with UV `[0, 0, 1, 1]`. For `Icon::Named { name, .. }`, look the name up in `sources.icons`: a region's UV is `region / sprite size`. A missing name is warned once per name hash: `engine_warn!(module: "text", "no text icon named {name:?}; leaving its space empty")`.
  - Alpha comes from the reveal (`reveal_alpha(icon.cluster, ..)`) times `place.opacity`.
  - The quad covers the icon rectangle mapped through the axes, unsnapped.
  - Push a sprite (`texture_of`, `SpriteLook` with `Filter::Default` and `place.draw.blend`, tint white × alpha, `set_last_sprite_scissor`).

The world-side functions in the same file:

- **`layout_world_text(world: &mut World, engine: &mut TextEngine)`**
  1. Call `engine.sync(world.resource::<Fonts>())`.
  2. For each `(Entity, &Text2d, Option<&TextLayoutState>)`, compute `key`: `Text2d`'s text, style and layout options hashed, plus `engine.generation()`. Collect the entities whose state is missing or whose `key` differs.
  3. For each, call `engine.layout(&text.text, &text.style, &text.layout, 1.0)`:
     - **On `Some(layout)`:** insert `TextLayoutState { layout, key }` and `TextBounds { size: layout.size, offset }`, with `offset = -anchor_fraction(text.layout.anchor) * layout.size`. Restart a `TextReveal` on the entity, if any, when its key changed (not on first layout).
     - **On `None`:** remove `TextLayoutState`, so the entity draws nothing; the engine warned already.
- **`anchor_fraction(anchor) -> Vec2`**: `TopLeft` (0, 0), `TopCenter` (0.5, 0), `TopRight` (1, 0), `CenterLeft` (0, 0.5), `Center` (0.5, 0.5), and so on down to `BottomRight` (1, 1).
- **`emit_world_text(..)`.** Its parameters:
  - `world: &mut World`, `sources_fonts..`;
  - `cameras: &[CameraDraw]`;
  - `render_scale: f32`: render pixels per screen unit, `mapping.render_size().0 as f32 / mapping.screen_size().x`;
  - `alpha: f32`: the interpolation overstep, as `EntityDrawer::collect` takes;
  - `game_time: f32`;
  - the targets.

  For each `(Entity, &GlobalTransform, Option<&PreviousGlobalTransform>, &Text2d, &TextLayoutState, Option<&TextReveal>, Option<&RenderLayers>)`:
  1. **Transform.** `drawn = render_global(global, previous, alpha)` gives position, rotation and scale. A non-finite scale or a scale axis of 0 is warned once per entity (`"{entity}: Text2d scaled to zero or a non-finite size; not drawn"`) and skipped.
  2. **Axes.** `axis_x = Vec2::from_angle(rotation) * scale.x`, `axis_y = Vec2::from_angle(rotation).perp() * scale.y`. Check `perp`'s handedness: +y is down, so `axis_y` must be `(0, scale.y)` at rotation 0.
  3. **Origin.** `origin = position + axis_x * offset.x + axis_y * offset.y`, using the `TextBounds` offset.
  4. **For each camera whose layers intersect the entity's** (default `RenderLayers::default()`):
     - **Culling:** the block's corners mapped through the axes; skip it unless `camera.view.is_visible(min, max)`.
     - **`px_per_layout`** is `camera.view.scale * render_scale * scale.x.abs().max(scale.y.abs())`.
     - **`pixel_grid`** is `Some((camera.view.screen_to_world(camera.view.rect_min), camera.view.scale * render_scale))` when `rotation == 0`, `camera.view.rotation == 0` and both scales are positive; otherwise `None`.
     - **`draw`** is `TextDraw { kind: DrawKind::Text, z: text.z_order, lit: text.lit, only_camera: Some(camera.entity), layers, scissor: None, blend: text.blend }`.
     - **`reveal`** is `Some((reveal.shown(), reveal.fade * reveal.per_second))` when present.
     - **`seed`** is `entity.to_bits()`, `time` is `game_time`, `opacity` is 1, `size_steps` is `true`.
     - Then `emit_text(..)`.
- **`prewarm(fonts: &mut Fonts, engine: &TextEngine, targets: &mut TextTargets)`**: for each `Prewarm` from `fonts.take_prewarm()`, and each character with a glyph (`glyph_id`), build the key its font's render mode would use at that size. That is `fill` (bin 0), `pixel` or `msdf`. Then call `targets.sprites.glyph(..)`.

`DrawList::set_last_sprite_scissor(&mut self, scissor: Option<PixelRect>)` sets the scissor of the most recently pushed `DrawSprite` (a no-op if none).

- [ ] **Step 5: Wire it into the frame**

- **`Engine` fields:** add `text: TextEngine` (`TextEngine::new()`) and `glyph_raster: GlyphRasterizer` (`GlyphRasterizer::new()`), plus `text_warned: HashSet<u64>`.
- **`present_frame`**, right after `self.entity_drawer.collect(..)`:
  1. Call `layout_world_text(&mut self.world, &mut self.text)`.
  2. Build `TextSources` from `self.world`'s `Fonts`, `TextEffects` and `TextIcons`. Use `resource_scope` or clone the cheap resources if borrows clash. `TextEffects` is `Clone` and its map holds `Arc`s; `TextIcons` is a small map.
  3. Build `TextTargets` from `self.glyph_raster`, `gpu`, `rend.device()`, `rend.queue()`, `self.draw_list` and `self.text_warned`.
  4. Call `prewarm`, then `emit_world_text`, with `alpha` as `EntityDrawer::collect` receives it, `game_time` from `Time`, and `render_scale` from the `ScreenMapping` resource.
- **After the frame:** call `self.text.end_frame()` once `batch.prepare` has run.

If `World` borrows clash (queries over `&mut World` while holding `&Fonts`), collect the text entities' data into a `Vec` first (as `EntityDrawer` collects drawables), then emit.

- [ ] **Step 6: Run the tests**

Run: `cargo test --test e2e text::` and `cargo test -p ryo2d --lib text::world`
Expected: all pass.

When an e2e assertion fails, save the frame (`frame.save("target/text-debug.png")`) and look at it before changing anything. The thresholds are deliberately loose; a failing one usually means a placement error (y flipped, anchor wrong, glyph at the pen's wrong side).

Then run `cargo test -p ryo2d --lib steady_frames_create_no_bind_groups` after adding a `Text2d` to its `Busy` scene. Expected: 0 bind groups after warm-up.

- [ ] **Step 7: Docs**

**`src/core/text/AGENTS.md`**, append:

```markdown
## World text

- `Text2d` lays out (cached) when its text, style or options change or a
  font changes (`TextLayoutState::key`); `TextBounds` holds the block's size
  and its top-left offset from the position (unscaled).
- Glyphs are emitted per camera (`only_camera`): raster glyphs are made at
  the size that camera sees (snapped to `raster_size_step`) and land on
  whole pixels when nothing is rotated; MSDF glyphs scale. Sorted with
  sprites (`DrawKind::Text`, after entities at equal z); unlit by default.
- Shadows, outlines, fills are three passes over the block. Raster outlines
  are stroke bands (twice the width, centred); MSDF outline/shadow/glow are
  shader lanes, limited to half the font's range at its base size.
- `TextReveal` advances on game time (`PostUpdateSet::Extract`) from the
  previous frame's layout and writes `TextRevealed` once.
```

**`src/core/render/AGENTS.md`**: add world text to step 5 of "Building the Frame" (after entities): lay out changed `Text2d`s, prewarm, emit glyph quads per camera.

- [ ] **Step 8: Gate and commit**

```bash
git add -A
git commit -m "feat: world text" -m "- Text2d draws rich text on entities, sorted with sprites, sharp under zoom
- raster glyphs land on whole pixels; MSDF fonts get outlines, shadows and glow
- TextReveal shows text a character at a time and says when it is done"
```

### Task 8: The GPU UI (commands, UI camera, `ui` pass, scale and resolution; CPU path removed)

**Files:**
- Create: `src/core/ui/immediate/commands.rs`, `src/core/render/ui.rs` (re-export `UiSpace`, `UiResolution` and `UI_CAMERA` from `render/mod.rs`), `src/core/ui/test_util.rs`, `tests/e2e/ui_drawing.rs`
- Modify:
  - **UI:**
    - `src/core/ui/immediate/context.rs`: recording instead of rasterizing; the text input's caret goes through the text engine.
    - `src/core/ui/immediate/mod.rs`, `src/core/ui/mod.rs`.
    - `src/core/ui/immediate/input.rs`: `UiInput::with_mouse_scale`.
    - `src/core/ui/retained/text_input.rs`: `ensure_cursor_visible` takes pixel positions.
    - `src/core/ui/immediate/font.rs`: **deleted**.
  - **Engine:**
    - `src/core/engine/mod.rs`:
      - fields: `ui_font` and `ui_buffer` out; `ui_commands`, `ui_space`, `ui_scale: Option<f32>` and `ui_resolution` in;
      - `EngineBuilder::ui_scale(Option<f32>)` and `EngineBuilder::ui_resolution(UiResolution)`;
      - `apply_mapping`.
    - `src/core/engine/frame.rs` (`draw_ui`), `src/core/engine/window.rs` (UI-scale requests), `src/core/engine/extract.rs` (the UI camera and its items).
    - `src/core/scene/context.rs` (`request_ui_scale(Option<f32>)`).
  - **Renderer:**
    - `src/core/render/frame.rs` (`FrameScene`: no `ui_buffer`, plus `ui_camera` and `ui_size`; `FrameInfo::ui`), `src/core/render/passes/output.rs` (the `ui` pass draws).
    - `src/core/graphics/compositor.rs` (the UI image as a render target; a gamma-space blend in the composite shader; no upload; no blend pipeline), `src/core/graphics/renderer.rs` (`frame.ui`; the UI image size), `src/core/graphics/screen.rs` (`ui_size` goes).
    - `src/core/shaders/core/fullscreen_quad.wgsl`.
  - **Prelude and tests:**
    - `src/lib.rs`: prelude loses `BitmapFont` and `create_debug_font`, gains `UiResolution`.
    - `tests/e2e.rs`, plus the existing UI tests in `src/core/ui/immediate/context.rs`, `src/core/engine/frame.rs`, `src/core/scene/manager.rs` and `tests/e2e/headless_engine.rs`.

**Interfaces:**
- Consumes:
  - `TextEngine::{layout, measure, line_height, caret_x, byte_at, selection_x, wrap_lines}`;
  - `emit_text`, `TextPlacement`, `TextDraw`;
  - `DrawList::{push_shape, push_glyph, set_last_sprite_scissor}`, `DrawShape`;
  - `BatchShader::UiRect`, `CameraDraw::ui`, `UI_FORMAT`, `UI_GAMMA` (Tasks 2-7).
- Produces:
  - **Public:**
    - `UiResolution::{Render, Window}` (default `Render`), with `EngineBuilder::ui_resolution(UiResolution)`;
    - `EngineBuilder::ui_scale(Option<f32>)` and `SceneContext::request_ui_scale(Option<f32>)`;
    - `UiContext::ui_scale() -> f32`;
    - on `UiContext`:
      - `draw_rounded_rect(rect, radius, color)`;
      - `draw_rect_bordered(rect, fill, radius, border_width, border_color)`;
      - `draw_nine_slice(sprite: &SpriteHandle, rect, insets: [f32; 4], tint: Color)`;
      - `draw_sprite(sprite: &SpriteHandle, x, y)`, `draw_sprite_scaled(&SpriteHandle, x, y, scale)`, `draw_sprite_rect(&SpriteHandle, rect, tint)`;
      - `draw_line(points: &[Vec2], width, color)`;
      - `draw_text_styled(text: impl Into<RichText>, x, y, &TextStyle)`;
      - `measure_text(text: impl Into<RichText>, &TextStyle) -> Vec2`, `line_height(&TextStyle) -> f32`;
    - `TextInputState::ensure_cursor_visible(&mut self, cursor_x: f32, width: f32)`.
  - **Removed from the prelude:** `BitmapFont`, `create_debug_font`. **Removed from `UiContext`:** the `draw_sprite_handle*` pair (`draw_sprite` takes a handle now) and `text_size_unscaled`.
  - **Crate-internal:**
    - `UiCommand` and `UiCommands { commands: Vec<UiCommand> }` (with `clear`, `is_empty`, `iter`);
    - `UiSpace { size: Vec2, px_scale: f32, image: (u32, u32), mouse_scale: f32 }`, with `UiSpace::of(mapping: &ScreenMapping, ui_scale: Option<f32>, resolution: UiResolution) -> UiSpace`;
    - `UI_CAMERA: Entity` (`Entity::PLACEHOLDER`);
    - `render::ui::emit_ui(..)`.
  - **Constructor:** `UiContext::new(commands: &'a mut UiCommands, text: &'a mut TextEngine, space: UiSpace, input: UiInput) -> UiContext<'a>`, which replaces `new`, `new_scaled`, `with_theme` and `with_theme_scaled`. `with_theme(self, theme: &'a Theme) -> Self` becomes a builder.

**The UI space.** UI units are what `Scene::ui` coordinates mean. With the automatic scale they equal screen units, so existing coordinates keep their meaning.

| Field | Meaning |
|---|---|
| `image` | The UI image size in render pixels. `Render` (the default): `mapping.render_size()`, which is physical pixels in the default presentation and the target in pixel art. `Window` in pixel art: the physical screen rectangle, `round(screen_size × mapping.scale())`. In the default presentation `Window` equals `Render`. |
| `px_scale` | Render pixels per UI unit. `ui_scale` when it is `Some(s)` with `s` finite and `> 0`. Otherwise automatic: `image.x / screen_size.x`, which is the scale factor in the default presentation, 1 on the pixel-art target, and the whole-pixel scale for `Window` in pixel art. An invalid `Some` (0, negative, NaN) is warned once and treated as automatic. |
| `size` | `image / px_scale`, in UI units. |
| `mouse_scale` | UI units per screen unit, `(image.x / screen_size.x) / px_scale`. 1 with the automatic scale. |

- [ ] **Step 1: Write the failing tests**

**`render/ui.rs`, the UI-space maths (pure):**

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::{Presentation, ScreenMapping};

  #[test]
  fn the_automatic_scale_keeps_ui_units_equal_to_screen_units() {
    let mapping = ScreenMapping::new(Presentation::ResolutionIndependent, (2560, 1440), 2.0);
    let space = UiSpace::of(&mapping, None, UiResolution::Render);
    assert_eq!(space.image, (2560, 1440));
    assert_eq!(space.px_scale, 2.0);
    assert_eq!(space.size, Vec2::new(1280.0, 720.0));
    assert_eq!(space.mouse_scale, 1.0);
  }

  #[test]
  fn a_ui_scale_makes_the_ui_bigger_and_converts_the_mouse() {
    let mapping = ScreenMapping::new(Presentation::ResolutionIndependent, (1280, 720), 1.0);
    let space = UiSpace::of(&mapping, Some(2.0), UiResolution::Render);
    assert_eq!(space.size, Vec2::new(640.0, 360.0));
    assert_eq!(space.mouse_scale, 0.5);
  }

  #[test]
  fn pixel_art_draws_on_the_target_or_the_window() {
    let mapping = ScreenMapping::new(Presentation::PixelArt { width: 320, height: 180 }, (1280, 720), 1.0);
    let target = UiSpace::of(&mapping, None, UiResolution::Render);
    assert_eq!((target.image, target.px_scale, target.size), ((320, 180), 1.0, Vec2::new(320.0, 180.0)));
    let window = UiSpace::of(&mapping, None, UiResolution::Window);
    assert_eq!((window.image, window.px_scale, window.size), ((1280, 720), 4.0, Vec2::new(320.0, 180.0)));
    assert_eq!(window.mouse_scale, 1.0);
  }

  #[test]
  fn a_bad_ui_scale_falls_back_to_automatic() {
    let mapping = ScreenMapping::new(Presentation::ResolutionIndependent, (1280, 720), 1.0);
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
      let space = UiSpace::of(&mapping, Some(bad), UiResolution::Render);
      assert_eq!(space.px_scale, 1.0, "{bad}");
    }
  }
}
```

**`context.rs`, recording.** The old pixel tests (`draw_text_at_2x_scale_draws_larger`, the clip pixel asserts) are replaced by these command tests. Shared test helpers go in a new `src/core/ui/test_util.rs` (`#[cfg(test)] pub(crate) mod test_util;` in `ui/mod.rs`). The console and scene-manager tests use them too (Tasks 9-10):

```rust
//! Recording UI contexts for tests (no GPU)

use glam::Vec2;

use crate::core::render::UiSpace;
use crate::core::text::{Fonts, TextEngine};
use crate::core::ui::UiInput;
use crate::core::ui::immediate::{UiCommands, UiContext};

/// A text engine over the built-in fonts
pub(crate) fn text_engine() -> TextEngine {
  let mut engine = TextEngine::new();
  engine.sync(&Fonts::with_builtins());
  engine
}

/// A context recording into `commands`, `size` UI units at scale 1
pub(crate) fn context<'a>(commands: &'a mut UiCommands, text: &'a mut TextEngine, size: (f32, f32)) -> UiContext<'a> {
  let space = UiSpace {
    size: Vec2::new(size.0, size.1),
    px_scale: 1.0,
    image: (size.0.ceil() as u32, size.1.ceil() as u32),
    mouse_scale: 1.0,
  };
  UiContext::new(commands, text, space, UiInput::default())
}
```

In `context.rs`'s tests, `use crate::core::ui::test_util::{context as ui_context, text_engine};` and a local `fn context<'a>(c: &'a mut UiCommands, t: &'a mut TextEngine) -> UiContext<'a> { ui_context(c, t, (320.0, 180.0)) }`, which the tests below call.

  #[test]
  fn drawing_records_commands_in_order() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    ui.draw_rect(Rect::new(1.0, 2.0, 3.0, 4.0), Color::RED);
    ui.draw_text("hi", 5.0, 6.0, Color::WHITE);
    drop(ui);
    let kinds: Vec<&str> = commands.iter().map(UiCommand::kind_name).collect();
    assert_eq!(kinds, vec!["rect", "text"]);
  }

  #[test]
  fn clips_nest_without_a_limit_and_an_empty_pop_warns_once() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    for i in 0..40 {
      ui.push_clip(Rect::new(i as f32, 0.0, 200.0, 100.0));
    }
    ui.draw_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::RED);
    for _ in 0..42 {
      ui.pop_clip();
    }
    drop(ui);
    assert_eq!(commands.iter().next().and_then(UiCommand::clip), Some(Rect::new(39.0, 0.0, 161.0, 100.0)));
    let warned = crate::core::dev_log::get_entries().iter().filter(|e| e.message.contains("pop_clip with no clip")).count();
    assert_eq!(warned, 1);
  }

  #[test]
  fn measured_and_drawn_text_agree() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let (w, h) = ui.text_size("Measure me");
    let measured = ui.measure_text("Measure me", &TextStyle::default());
    assert_eq!((w, h), (measured.x.ceil() as u32, measured.y.ceil() as u32));
    ui.draw_text_anchored("Measure me", Anchor::Center, 0.0, 0.0, Color::WHITE);
    drop(ui);
    let origin = commands.iter().find_map(UiCommand::text_origin).unwrap();
    assert!((origin.x - (160.0 - measured.x / 2.0)).abs() < 0.01);
  }

  #[test]
  fn draw_rect_outline_is_one_bordered_command() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    ui.draw_rect_outline(Rect::new(0.0, 0.0, 20.0, 10.0), Color::WHITE, 2.0);
    drop(ui);
    assert_eq!(commands.iter().count(), 1);
  }
```

`UiCommand::kind_name`, `clip()` and `text_origin()` are `#[cfg(test)]` helpers on the enum. Port the remaining old context tests the same way. Most check input consumption and occlusion, which need no pixels: keep their logic and swap the constructor.

**`tests/e2e/ui_drawing.rs`, pixel checks** (mount it in `tests/e2e.rs`):

```rust
//! The GPU UI, pixel-checked

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

struct Ui<F: FnMut(&mut UiContext) + 'static> {
  draw: F,
  world: Option<fn(&mut SceneContext)>,
}

impl<F: FnMut(&mut UiContext) + 'static> Scene for Ui<F> {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    if let Some(setup) = self.world {
      setup(ctx);
    }
  }
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    (self.draw)(ui);
    SceneAction::Continue
  }
}

fn white_world(ctx: &mut SceneContext) {
  let white = SpriteHandle::from_sprite(Sprite::solid_rect(400, 400, 255, 255, 255, 255), "white");
  ctx.world.spawn((Transform::from_xy(0.0, 0.0), Renderable::sprite(white)));
}

fn frame(scale_factor: Option<f64>, builder: impl FnOnce(EngineBuilder) -> EngineBuilder, scene: impl Scene + 'static) -> Option<image::RgbaImage> {
  let base = builder(EngineBuilder::new().resolution(128, 96).start_scene(scene));
  let mut engine = match scale_factor {
    Some(f) => HeadlessEngine::new_or_skip_with_scale_factor(base, f)?,
    None => HeadlessEngine::new_or_skip(base)?,
  };
  engine.step_frames(2).unwrap();
  Some(engine.frame().unwrap())
}

#[test]
fn a_half_transparent_panel_has_its_specified_colour() {
  let scene = Ui { draw: |ui: &mut UiContext| ui.draw_rect(Rect::new(0.0, 0.0, 64.0, 96.0), Color::new(0, 0, 0, 128)), world: Some(white_world) };
  let Some(frame) = frame(None, |b| b, scene) else { return };
  let p = frame.get_pixel(32, 48).0;
  assert!((i32::from(p[0]) - 127).abs() <= 2, "gamma-space half black over white: {p:?}");
  assert_eq!(frame.get_pixel(96, 48).0, [255, 255, 255, 255]);
}

#[test]
fn faded_text_is_translucent() {
  let scene = Ui { draw: |ui: &mut UiContext| ui.draw_text_styled("HHHH", 8.0, 8.0, &TextStyle::new().size(32.0).color(Color::new(255, 255, 255, 128))), world: None };
  let Some(frame) = frame(None, |b| b, scene) else { return };
  let brightest = frame.pixels().map(|p| p.0[0]).max().unwrap();
  assert!((120..=136).contains(&brightest), "half-opaque white over black: {brightest}");
}

#[test]
fn text_at_twice_the_scale_factor_is_sharp() {
  let scene = Ui { draw: |ui: &mut UiContext| ui.draw_text_styled("Il|", 8.0, 8.0, &TextStyle::new().size(20.0)), world: None };
  let Some(frame) = frame(Some(2.0), |b| b, scene) else { return };
  let lit = frame.pixels().filter(|p| p.0[0] > 24).count();
  let solid = frame.pixels().filter(|p| p.0[0] > 230).count();
  assert!(lit > 50);
  assert!(solid * 10 >= lit * 5, "rendered at physical resolution: {solid} of {lit}");
}

#[test]
fn pixel_art_ui_draws_on_the_target_or_the_window() {
  let draw = |ui: &mut UiContext| ui.draw_rect(Rect::new(10.3, 10.3, 5.0, 5.0), Color::WHITE);
  let target = frame(None, |b| b.resolution(320, 180).pixel_art(80, 45), Ui { draw, world: None });
  let window = frame(None, |b| b.resolution(320, 180).pixel_art(80, 45).ui_resolution(UiResolution::Window), Ui { draw, world: None });
  let (Some(target), Some(window)) = (target, window) else { return };
  // frame() is the render-size composite: target-resolution UI has whole target pixels
  assert_eq!(target.dimensions(), (80, 45));
  let partial = target.pixels().filter(|p| p.0[0] > 10 && p.0[0] < 245).count();
  assert_eq!(partial, 0, "snapped to the target's pixels");
  let _ = window;
}

#[test]
fn a_bad_ui_scale_still_draws() {
  let scene = Ui { draw: |ui: &mut UiContext| ui.draw_rect(Rect::new(10.0, 10.0, 20.0, 20.0), Color::WHITE), world: None };
  let Some(frame) = frame(None, |b| b.ui_scale(Some(f32::NAN)), scene) else { return };
  assert_eq!(frame.get_pixel(20, 20).0, [255, 255, 255, 255]);
}
```

The Window-mode half of `pixel_art_ui_draws_on_the_target_or_the_window` needs the window image. Use `engine.window_frame()` and assert the rect's edges fall on whole window pixels at `10.3 * 4 = 41.2` (anti-aliased, so a partial column exists). Write that assertion once you see the window frame's layout. The test above shows the target half.

**A guard** in `src/core/graphics/compositor.rs`'s tests: the compositor never uploads pixels any more.

```rust
  /// The UI is drawn on the GPU: nothing writes a whole UI texture per frame
  #[test]
  fn the_compositor_uploads_no_pixels() {
    let source = include_str!("compositor.rs");
    let code = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(!code.contains("write_texture"));
  }
```

**`engine/frame.rs` test** `a_scene_switched_from_ui_draws_its_ui_the_same_frame`: rewrite it to inspect `e.ui_commands` instead of `e.ui_buffer[..4]`. After the switch frame, assert the commands hold exactly the new scene's draws, with no leftovers from the first pass.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui:: render::ui` and `cargo test --test e2e ui_drawing`
Expected: compile errors.

- [ ] **Step 3: Commands**

`src/core/ui/immediate/commands.rs`:

```rust
//! What `UiContext` records: drawn by the engine's UI pass through the 2D
//! batch, in submission order (popups last)

use std::sync::Arc;

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::text::TextLayout;
use crate::core::ui::{Color, Rect};

/// One UI draw, in UI units
#[derive(Debug, Clone)]
pub(crate) enum UiCommand {
  Rect { rect: Rect, fill: Color, radius: f32, border: Option<(f32, Color)>, clip: Option<Rect>, layer: u8 },
  Sprite { sprite: SpriteHandle, rect: Rect, uv: [f32; 4], tint: Color, clip: Option<Rect>, layer: u8 },
  Line { points: Vec<Vec2>, width: f32, color: Color, closed: bool, clip: Option<Rect>, layer: u8 },
  Text { layout: Arc<TextLayout>, origin: Vec2, reveal: Option<(f32, f32)>, opacity: f32, clip: Option<Rect>, layer: u8 },
}

/// The frame's UI draws (cleared each UI pass)
#[derive(Debug, Default)]
pub(crate) struct UiCommands {
  commands: Vec<UiCommand>,
}

impl UiCommands {
  pub(crate) fn clear(&mut self) {
    self.commands.clear();
  }
  pub(crate) fn push(&mut self, command: UiCommand) {
    self.commands.push(command);
  }
  pub(crate) fn is_empty(&self) -> bool {
    self.commands.is_empty()
  }
  pub(crate) fn iter(&self) -> impl Iterator<Item = &UiCommand> {
    self.commands.iter()
  }
}
```

Nine-slices are recorded as nine `Sprite` commands (UV sub-rects), so the renderer has one sprite path.

- [ ] **Step 4: `UiContext` records**

Rewrite `UiContext`'s fields:

```rust
pub struct UiContext<'a> {
  commands: &'a mut UiCommands,
  text: &'a mut TextEngine,
  space: UiSpace,
  input: UiInput,
  theme: Option<&'a Theme>,
  clip_stack: Vec<Rect>,
  current_clip: Option<Rect>,
  layer: u8,
  pending_dropdowns: Vec<PendingDropdown>,
  focused_dropdown_id: u32,
  keyboard_focus: bool,
  mouse_occluded: bool,
  dt: f32,
}
```

`UiContext::new(commands, text, space, input)` applies `input.with_mouse_scale(space.mouse_scale)`. `with_theme(self, theme)` sets the theme. `ui_scale()` returns `space.px_scale`. `screen_size()` keeps returning `(u32, u32)`: `space.size` rounded up.

Rewrite each primitive to record a command:

| Method | Records |
|---|---|
| `draw_rect(rect, color)` | `UiCommand::Rect { rect, fill: color, radius: 0.0, border: None, clip: self.current_clip, layer: self.layer }`. Skip non-finite or empty rects, and rects entirely outside the current clip. |
| `draw_rect_outline(rect, color, thickness)` | One `Rect` with `fill: Color::TRANSPARENT` and `border: Some((thickness, color))`. The shader's border is inside the rect, as the four old rects were. |
| `draw_panel(rect, bg, border)` | One `Rect` with `fill: bg` and `border: border.map(|c| (1.0, c))`. |
| `draw_rounded_rect`, `draw_rect_bordered` | `Rect` with the radius and border. |
| `draw_progress_bar*`, `draw_focus_ring*` | Built from the above, as before. |
| `draw_modal_overlay` | Clip `None`, the whole UI size. |
| `draw_sprite(sprite: &SpriteHandle, x, y)` | `Sprite { rect: Rect::new(x, y, w, h), uv: [0.0, 0.0, 1.0, 1.0], tint: Color::WHITE, .. }`, with `w` and `h` from `sprite.width()` and `height()`. |
| `draw_sprite_scaled(.., scale)` | The same with `w * scale` and `h * scale`. |
| `draw_sprite_rect(sprite, rect, tint)` | `Sprite` with the given rect and tint. |
| `draw_nine_slice(sprite, rect, [left, top, right, bottom], tint)` | Nine `Sprite`s: the corners at their pixel size, the edges stretched along one axis, the centre stretched both ways. UVs are insets divided by the sprite's size. |
| `draw_line(points, width, color)` | `Line { closed: false, .. }`, skipped for fewer than 2 points or a non-finite or non-positive width. |

Text:

- **`draw_text(text, x, y, color)`:** `self.text.layout(&text.into(), &TextStyle::default().color(color), &TextLayoutOptions::default(), self.space.px_scale)`. On `Some(layout)`, push `UiCommand::Text { layout, origin: Vec2::new(x, y), reveal: None, opacity: 1.0, clip, layer }`. The origin is the block's top-left, as the old y was the top of the cell. Newlines now break lines (the old rasterizer skipped them).
- **`draw_text_styled(text, x, y, style)`:** the same with the given style.
- **`text_size(text) -> (u32, u32)`:** `measure_text(text, &TextStyle::default())` rounded up.
- **`measure_text(text, style) -> Vec2`:** `self.text.measure(&text.into(), style, &TextLayoutOptions::default(), self.space.px_scale)`.
- **`line_height(style)`:** `self.text.line_height(style, self.space.px_scale)`.
- **`draw_text_anchored(text, anchor, ox, oy, color)`:** measure, then `anchor.calculate_position(ox, oy, w, h, screen_w, screen_h)`, then `draw_text`. This fixes the old unscaled measurement.
- **`draw_text_centered(text, rect, color)`:** measure. If the text is wider than the rect, draw it wrapped and centred (as before); otherwise draw it at `rect.center() - size / 2`.
- **`wrap_text(text, max_width: u32) -> Vec<String>`:** `self.text.wrap_lines(text, &TextStyle::default(), max_width as f32, px_scale)`, mapped to owned strings with trailing whitespace trimmed.
- **`wrapped_text_size(text, max_width)`:** measure with `TextLayoutOptions::default().wrap(max_width as f32)`, rounded up.
- **`draw_text_wrapped(text, x, y, max_width, color)`:** layout with `wrap(max_width)` and record one text command.
- **`draw_text_wrapped_centered(text, rect, color)`:** layout with `wrap(rect.width).align(TextAlign::Center)`, centred vertically in the rect.
- **`label(text, x, y, color, bg)`:** a background rect padded 4 around the measured size, then `draw_text`.

Clip:
- The stack is a plain `Vec`; delete `MAX_CLIP_STACK_DEPTH` and both `assert!`s.
- `pop_clip` on an empty stack warns once (a `static AtomicBool`): `engine_warn!(module: "ui", "pop_clip with no clip to pop; ignored")`.
- `update_current_clip` intersects as before.

The text input (`text_input`, context.rs:1301-1510): every `char_index * char_width` becomes a text-engine call on the same style the field draws with (default style, `text_color`):
- **Click and drag to a byte:** `self.text.byte_at(text, &style, mouse_x - text_x + scroll, px_scale)`, then `state.set_cursor(byte, select)`. Add a byte-index `set_cursor(&mut self, byte: usize, select: bool)` to `TextInputState`, which snaps to a char boundary with `str::floor_char_boundary`, or by walking back while `!is_char_boundary`.
- **Caret x:** `self.text.caret_x(text, &style, cursor_byte, px_scale)`.
- **Selection:** `self.text.selection_x(text, &style, start..end, px_scale)` gives the selection rect's x span.
- **Preedit:** placed at the caret x; its width is `measure_text(preedit)`.
- **Vertical:** `text_y = rect.y + (rect.height - line_height) / 2`, and the caret is `line_height` tall.
- **Scroll:** `state.ensure_cursor_visible(caret_x, text_width)`. The new body:

```rust
  /// Keep the caret (`cursor_x`, pixels from the text's start) inside a field
  /// `width` pixels wide, scrolling as little as possible
  pub fn ensure_cursor_visible(&mut self, cursor_x: f32, width: f32) {
    let margin = 2.0;
    if cursor_x - self.scroll_offset > width - margin {
      self.scroll_offset = cursor_x - width + margin;
    } else if cursor_x < self.scroll_offset {
      self.scroll_offset = (cursor_x - margin).max(0.0);
    }
  }
```

  Update its tests in `text_input.rs` (they passed `char_width`) to pass pixel positions.

The console's caret is measurement-based already and keeps working.

Delete `src/core/ui/immediate/font.rs` and its re-exports (`ui/mod.rs`, `ui/immediate/mod.rs`, the prelude). Fix the `scene/manager.rs` test's context construction with the new constructor.

`UiInput::with_mouse_scale(self, scale: f32) -> Self` stores the factor. `mouse_position`, `mouse_x` and `mouse_y` return positions multiplied by it.

- [ ] **Step 5: The engine side**

`src/core/engine/mod.rs`:
- **`EngineConfig`:** `ui_scale: Option<f32>` (default `None`; was `Some(1)`) and `ui_resolution: UiResolution` (default `Render`).
- **`EngineBuilder`:**
  - `ui_scale(mut self, scale: Option<f32>) -> Self`, documented as "render pixels per UI unit; `None` (the default) keeps UI units equal to screen units at any display scale";
  - `ui_resolution(mut self, resolution: UiResolution) -> Self`.
- **Fields:** remove `ui_font`, `ui_buffer` and the `u32` `ui_scale`. Add `ui_commands: UiCommands` and `ui_space: UiSpace` (recomputed in `apply_mapping` and when a UI-scale request applies).
- **`window.rs` `apply_ui_scale`:** takes `Option<f32>`. `request_ui_scale(Option<f32>)` stores `Some(scale)` or `None` in the `WindowRequest` (replace the old 0-means-auto encoding with an `Option<Option<f32>>` field). `ScaleFactorChanged` recomputes `ui_space`.

`draw_ui` (`engine/frame.rs`):

```rust
  fn draw_ui(&mut self) -> Option<SceneAction> {
    self.renderer.as_ref()?;
    self.ui_commands.clear();
    self.text.sync(self.world.resource::<crate::core::text::Fonts>());
    let snapshot = UiInput::snapshot(self.world.resource::<Input>());
    let real_dt = self.world.resource::<Time>().real.delta();
    let space = self.ui_space;
    let (action, consumed) = {
      let mut ui = UiContext::new(&mut self.ui_commands, &mut self.text, space, snapshot);
      ui.set_dt(real_dt);
      // ... unchanged: console occlusion, scenes' ui, overlays, console ...
      (action, ui.into_input())
    };
    // ... unchanged: apply_consumed and ui_mouse_capture ...
    Some(action)
  }
```

`overlays::render_*` take `screen_height` as `u32` today. Pass `space.size.y.ceil() as u32` (Task 10 reworks them). The console's `render(&mut ui, width, height)` likewise.

- [ ] **Step 6: The renderer side**

**`src/core/render/ui.rs`.** It holds `UiSpace`, `UiResolution` (public, `#[non_exhaustive]`, `Default = Render`), `UiSpace::of` (as the table), `UI_CAMERA` and:

```rust
/// Lay the frame's UI commands into the draw list for the UI camera and
/// return that camera, or `None` when nothing was drawn
pub(crate) fn emit_ui(
  commands: &UiCommands,
  space: &UiSpace,
  pixel_art: bool,
  sources: &TextSources<'_>,
  targets: &mut TextTargets<'_>,
  triangles: &mut Triangles,
  real_time: f32,
) -> Option<CameraDraw> {
  if commands.is_empty() {
    return None;
  }
  let view = CameraView::compute(space.size / 2.0, 0.0, &Camera2d::default(), Vec2::ZERO, space.size);
  for (index, command) in commands.iter().enumerate() {
    // Painter order: z orders the layer, then submission
    let z = (i32::from(command_layer(command)) << 24) | index as i32;
    // ... one arm per command, below ...
  }
  Some(CameraDraw {
    entity: UI_CAMERA,
    view,
    layers: RenderLayers::all(),
    clear: CameraClear::None,
    target: None,
    direct: false,
    own_image: false,
    rect: PixelRect { x: 0, y: 0, width: space.image.0, height: space.image.1 },
    lit: false,
    ui: true,
  })
}
```

Write the command arms. All push unlit items with `only_camera: Some(UI_CAMERA)`. `scissor` comes from `clip` converted to image pixels (`floor` of the min corner times `px_scale`, `ceil` of the max corner, clamped to the image; an empty clip skips the command).

- **`Rect`:**
  - Instance: `SpriteInstance::new().with_transform(centre.x, centre.y, 0.0, w, h).with_tint(fill, 1.0)`, with `effect_params = [radius, border_width, w, h]` and `effect_extra = border colour / 255` (sRGB, straight).
  - Push `DrawShape { instance, blend: BlendMode::Alpha, layers: RenderLayers::all(), min, max, scissor }` with `DrawKind::Sprite` and `z`.
  - A fully transparent fill with no border is skipped.
- **`Sprite`:** `targets.sprites.texture_of(&sprite)`, an instance from rect and UV, `SpriteLook { shading: Plain, blend: Alpha, filter: Filter::Default, lit: false }`, `push_sprite_in(DrawKind::Sprite, z, ..)`, then `set_last_sprite_scissor`.
- **`Line`:** points into `PolyPoint`s (`half_width = width / 2`, `line_color(color, 1.0)`), then `push_polyline` with `LineLook { lit: false, z, texture: None, blend: Alpha, filter: Default, layers: RenderLayers::all(), only_camera: Some(UI_CAMERA) }`. Add a `set_last_triangles_scissor` to `DrawList`, as for sprites, and call it.
- **`Text`:** `emit_text(sources, targets, &layout, &TextPlacement { .. })` with:
  - `origin`, `axis_x: Vec2::new(1.0 / space.px_scale, 0.0)` and `axis_y: Vec2::new(0.0, 1.0 / space.px_scale)`;
  - `px_per_layout: 1.0`, `size_steps: false`, `pixel_grid: Some((Vec2::ZERO, space.px_scale))`;
  - `draw: TextDraw { kind: DrawKind::Sprite, z, lit: false, only_camera: Some(UI_CAMERA), layers: RenderLayers::all(), scissor, blend: BlendMode::Alpha }`;
  - `time: real_time`, `reveal`, `seed: index as u64`, `opacity`.

  Every UI item uses one `DrawKind` (`Sprite`), so the sort's kind key never reorders them; `z` does the ordering.

**Extract** (`present_frame`), after world text:
1. Call `emit_ui(&self.ui_commands, &self.ui_space, pixel_art, ..)`.
2. If it returns a camera, keep it in `self.ui_camera` and add its `CameraGlobals` to the camera uniforms. `snap` is `pixel_art && the UI image is the target`; `resolution` is `space.image`.
3. Sync the batch's camera groups.
4. Give `batch.prepare` the world cameras followed by the UI camera. Make a scratch `Vec<CameraDraw>` field and refill it each frame; do not add the UI camera to `self.camera_draws`, which the camera passes iterate.
5. `FrameScene` gets `ui_camera: Option<&CameraDraw>` and `ui_size: (u32, u32)` (the image size), and loses `ui_buffer`.

**`FrameInfo::ui`** is `scene.ui_camera.is_some()` (`renderer.rs:785`; the scan goes).

**The compositor.** The UI texture becomes the UI image: `Rgba8Unorm`, usage `RENDER_ATTACHMENT | TEXTURE_BINDING`, label `"UI Image"`. Changes:
- recreate it when the UI image size changes (`resize_ui(device, w, h)`, called from `set_mapping` and when `ui_space` changes);
- `ui_view()` returns its view;
- delete `upload_ui` and the blend pipeline;
- one pipeline (`REPLACE`) whose bind group layout gains a binding 3 for the UI texture (filterable float, fragment);
- `draw(.., include_ui, ..)` binds the UI image when `include_ui` is true, and a 1×1 transparent `Rgba8Unorm` texture (made once in `new`) otherwise;
- the `ViewportUniform` flags: `flags.y` is 1.0 when the UI is included.

  Keep one uniform buffer per (viewport, ui) pair: add two buffers alongside the two that exist, written in `set_viewport_transform` and at construction. Choose the buffer in `draw` from `(viewport, include_ui)`.

`fullscreen_quad.wgsl` becomes:

```wgsl
@group(0) @binding(3) var t_ui: texture_2d<f32>;

fn decode_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(t_game, s_game, in.tex_coord);
    if viewport.flags.y > 0.5 {
        // The UI image holds gamma-encoded premultiplied colour: blend it
        // over the frame in sRGB space, as the web does
        let ui = textureSample(t_ui, s_game, in.tex_coord);
        if ui.a > 0.0 {
            let frame = encode_srgb(vec4<f32>(color.rgb, 1.0)).rgb;
            color = vec4<f32>(decode_srgb(ui.rgb + frame * (1.0 - ui.a)), 1.0);
        }
    }
    if viewport.flags.x > 0.5 {
        return encode_srgb(color);
    }
    return color;
}
```

(`encode_srgb` already exists in the file. The display image is sampled as linear because it is an sRGB texture; the UI image is not, so its texels are the gamma values.)

**The `ui` pass** (`passes/output.rs`):

```rust
impl RenderPass for Ui {
  fn id(&self) -> &str {
    pass_ids::UI
  }
  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.ui
  }
  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (Some(camera), Some(batch)) = (ctx.scene.ui_camera, ctx.scene.batch) else {
      return;
    };
    let sprites = ctx.scene.sprites;
    let image = ctx.scene.ui_size;
    let draws = batch.draws(camera.entity, false);
    let view = ctx.targets.compositor.ui_view();
    let mut pass = ctx.encoder().begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("UI Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
      })],
      depth_stencil_attachment: None,
      timestamp_writes: None,
      occlusion_query_set: None,
      multiview_mask: None,
    });
    for draw in draws {
      batch.draw(&mut pass, draw, camera.entity, sprites, image);
    }
    drop(pass);
    ctx.ui_uploaded = true;
  }
}
```

(Rename `ui_uploaded` to `ui_drawn` throughout. Match the `RenderPassDescriptor` fields to what `world.rs` uses for wgpu 30.) `Composite` and `Capture` pass `ctx.ui_drawn` as `include_ui`, as they pass `ui_uploaded` now.

`ScreenMapping::ui_size` and `WgpuRenderer::ui_size` go; the UI image size comes from `UiSpace`, which the engine hands the renderer with the mapping (`set_mapping(mapping, ui_image)`).

- [ ] **Step 7: Run the tests**

Run, in order:
1. `cargo test -p ryo2d --lib ui:: render::ui engine::frame scene::manager`. Expected: pass.
2. `cargo test --test e2e`. Expected: every UI test that passed before still passes, with exact colours for opaque rects (`sprite_and_ui_appear_in_frame`, `transition_covers_trails_not_ui`, `ui_is_untouched_by_effects_and_lighting`), and the new `ui_drawing` tests pass.
3. `requested_ui_scale_applies_headless` (`headless_engine.rs:305`) asserted that rect coordinates are not scaled. Update it: with `request_ui_scale(Some(2.0))` the rect is drawn at twice the size, so it covers (35, 35). It now pins the new meaning.
4. `cargo test -p demo-kit`. Expected: pass. Its backing thresholds hold with gamma-space blending. If one fails, look at the frame (`frame.save`) before changing a threshold, and report the numbers.

- [ ] **Step 8: Docs**

- **`src/core/ui/AGENTS.md`:** replace the CPU-rasterizer description. `UiContext` records `UiCommand`s in UI units. The engine's `ui` pass draws them through the 2D batch into the UI image (`Rgba8Unorm`, gamma-encoded premultiplied). The composite blends it in sRGB space. Clips are scissors with no depth limit. The text input's geometry comes from the text engine.
- **`src/core/ui/immediate/AGENTS.md`:** the clip stack note (unbounded, empty pop warned).
- **`src/core/render/AGENTS.md`:**
  - the pass table's `UI` row: "draws the frame's UI commands into the UI image (the UI camera, `UI_FORMAT`)";
  - the `COMPOSITE` row: "UI blended last in sRGB space";
  - the "UI is composited only if the `ui` pass drew this frame" decision (renamed from uploaded).
- **`src/core/graphics/AGENTS.md`:** the rendering diagram's UI line, `Scene::ui(ui, ctx) ─→ UiContext ─→ UiCommands ─→ UI camera (batch) ─→ UI image ─→ composited last`, and delete "The only CPU drawing left is the UI".

- [ ] **Step 9: Gate and commit**

```bash
git add -A
git commit -m "feat: the UI draws on the GPU" -m "- UiContext records commands; the batch draws them into a UI image the composite blends in sRGB space
- crisp at any display scale; translucent panels and faded text get their specified colours
- ui_scale is render pixels per UI unit; pixel-art games pick target or window resolution
- clipping is a scissor stack with no depth limit; the 8x8 bitmap font is gone"
```

### Task 9: Widgets on the new text (`ui.text`, dropdown popups, measured layouts)

**Files:**
- Modify: `src/core/ui/immediate/context.rs` (the `UiText` builder, `wrap_text_styled`, dropdown popups, the 8 px assumptions), `src/core/scene/manager.rs` (run the popup flush after each scene's `ui`), `src/core/ui/immediate/mod.rs`, `src/lib.rs` (prelude: `UiText`), `src/core/ui/AGENTS.md`

**Interfaces:**
- Consumes: Task 8's `UiContext` (recording, `self.text`, `self.space`, `layer`), `UiCommand::Text { reveal, opacity, .. }`, `RichText`, `TextStyle`, `TextLayoutOptions`, `TextAlign`.
- Produces:
  - `UiContext::text<'u>(&'u mut self, text: impl Into<RichText>) -> UiText<'u, 'a>`, with:
    - `at(x, y)`, `style(&TextStyle)`, `size(f32)`, `color(Color)`;
    - `wrap(width)`, `align(TextAlign)`, `max_lines(u32)`, `ellipsis()`;
    - `anchor(Anchor)`, which places the block's anchor point at `(x, y)`;
    - `reveal(shown: f32)` and `fade(clusters: f32)`;
    - `opacity(f32)`;
    - `draw(self) -> Vec2`, which returns the size in UI units.
  - `UiContext::wrap_text_styled(&mut self, text: &str, width: f32, style: &TextStyle) -> Vec<String>`.
  - `finish_dropdowns` becomes `pub(crate)`. The engine runs it after each scene's `ui`, and it draws on the popup layer (1).

- [ ] **Step 1: Write the failing tests**

In `context.rs`'s tests (using Task 8's `context` and `text_engine` helpers):

```rust
  #[test]
  fn the_text_builder_records_its_options() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let size = ui
      .text(RichText::markup("[b]Hi[/b] there"))
      .at(100.0, 50.0)
      .anchor(Anchor::Center)
      .reveal(3.0)
      .opacity(0.5)
      .draw();
    drop(ui);
    let UiCommand::Text { origin, reveal, opacity, .. } = commands.iter().next().unwrap().clone() else { panic!() };
    assert!((origin - (Vec2::new(100.0, 50.0) - size / 2.0)).length() < 0.01);
    assert_eq!(reveal, Some((3.0, 0.0)));
    assert_eq!(opacity, 0.5);
  }

  #[test]
  fn wrapped_text_through_the_builder_fits_its_width() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let size = ui.text("a few words that need to wrap").at(0.0, 0.0).wrap(60.0).draw();
    assert!(size.x <= 60.5);
    assert!(size.y > 30.0, "more than one line");
  }

  #[test]
  fn open_dropdown_lists_draw_on_the_popup_layer_after_the_page() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let mut state = DropdownState::new();
    state.open();
    ui.dropdown(&mut state, Rect::new(10.0, 10.0, 100.0, 20.0), &["one", "two"]);
    ui.draw_rect(Rect::new(0.0, 0.0, 300.0, 300.0), Color::RED); // the page draws on after
    ui.finish_dropdowns();
    drop(ui);
    let layers: Vec<u8> = commands.iter().map(UiCommand::layer).collect();
    let first_popup = layers.iter().position(|&l| l == 1).expect("a popup command");
    assert!(layers[first_popup..].iter().all(|&l| l == 1));
    assert!(layers[..first_popup].contains(&0));
  }

  #[test]
  fn dropdown_text_is_centred_by_the_real_line_height() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text);
    let mut state = DropdownState::new();
    let line = ui.line_height(&TextStyle::default());
    ui.dropdown(&mut state, Rect::new(10.0, 10.0, 100.0, 30.0), &["one"]);
    drop(ui);
    let origin = commands.iter().find_map(UiCommand::text_origin).unwrap();
    assert!((origin.y - (10.0 + (30.0 - line) / 2.0)).abs() < 0.01);
  }
```

`UiCommand::layer()` is another `#[cfg(test)]` helper. In `scene/manager.rs`'s tests, add one checking that a scene whose `ui` opens a dropdown and never calls `finish_dropdowns` still gets its list drawn (popup commands present after `ui_pass`).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::immediate scene::manager`
Expected: compile errors (`text`, `layer` not found) and the dropdown centring assertion failing.

- [ ] **Step 3: The builder**

```rust
/// A text draw being set up; `draw()` records it
pub struct UiText<'u, 'a> {
  ui: &'u mut UiContext<'a>,
  text: RichText,
  at: Vec2,
  style: TextStyle,
  options: TextLayoutOptions,
  reveal: Option<(f32, f32)>,
  opacity: f32,
}

impl<'a> UiContext<'a> {
  /// Draw text with every option: `ui.text("Hi").at(10.0, 10.0).size(24.0).draw()`
  pub fn text<'u>(&'u mut self, text: impl Into<RichText>) -> UiText<'u, 'a> {
    UiText {
      ui: self,
      text: text.into(),
      at: Vec2::ZERO,
      style: TextStyle::default(),
      options: TextLayoutOptions::default(),
      reveal: None,
      opacity: 1.0,
    }
  }
}

impl UiText<'_, '_> {
  pub fn at(mut self, x: f32, y: f32) -> Self {
    self.at = Vec2::new(x, y);
    self
  }
  pub fn style(mut self, style: &TextStyle) -> Self {
    self.style = style.clone();
    self
  }
  pub fn size(mut self, size: f32) -> Self {
    self.style.size = size;
    self
  }
  pub fn color(mut self, color: Color) -> Self {
    self.style.color = color;
    self
  }
  pub fn wrap(mut self, width: f32) -> Self {
    self.options.wrap = Some(width);
    self
  }
  pub fn align(mut self, align: TextAlign) -> Self {
    self.options.align = align;
    self
  }
  pub fn max_lines(mut self, lines: u32) -> Self {
    self.options.max_lines = Some(lines);
    self
  }
  pub fn ellipsis(mut self) -> Self {
    self.options.overflow = TextOverflow::Ellipsis;
    self
  }
  /// Which point of the block sits at `at` (default the top-left)
  pub fn anchor(mut self, anchor: Anchor) -> Self {
    self.options.anchor = anchor;
    self
  }
  /// Show the first `shown` character clusters (fractional while fading)
  pub fn reveal(mut self, shown: f32) -> Self {
    let fade = self.reveal.map_or(0.0, |(_, fade)| fade);
    self.reveal = Some((shown, fade));
    self
  }
  /// Revealed characters fade in over this many clusters
  pub fn fade(mut self, clusters: f32) -> Self {
    let shown = self.reveal.map_or(f32::MAX, |(shown, _)| shown);
    self.reveal = Some((shown, clusters));
    self
  }
  pub fn opacity(mut self, opacity: f32) -> Self {
    self.opacity = opacity;
    self
  }

  /// Record the text; its size in UI units
  pub fn draw(self) -> Vec2 {
    let scale = self.ui.space.px_scale;
    let Some(layout) = self.ui.text.layout(&self.text, &self.style, &self.options, scale) else {
      return Vec2::ZERO;
    };
    let size = layout.size / scale;
    let origin = self.at - anchor_fraction(self.options.anchor) * size;
    let (clip, layer) = (self.ui.current_clip, self.ui.layer);
    self.ui.commands.push(UiCommand::Text { layout, origin, reveal: self.reveal, opacity: self.opacity, clip, layer });
    size
  }
}
```

`anchor_fraction` is Task 7's (in `render/text.rs`). Move it to `src/core/ui/types.rs` as `Anchor::fraction(self) -> Vec2` so both modules use it, and update Task 7's call site.

`UiText` is `#[must_use = "call .draw() to draw the text"]`.

- [ ] **Step 4: Dropdowns and the 8 px assumptions**

- **Popup flush.** `finish_dropdowns` becomes `pub(crate) fn finish_dropdowns(&mut self)`. It sets `self.layer = 1` while drawing, then restores it. In `SceneManager::ui_pass`, call `ui.finish_dropdowns()` right after each `slot.call(.. scene.ui(ui, ctx))`, so a scene's open lists draw above everything it drew. The engine's console and overlays draw after all scenes, on layer 0, with higher sequence numbers; layer 1 still beats them. That is the old behaviour: open lists covered everything.
- **Callers.** Remove the public `finish_dropdowns()` call from `examples/demos/ui/src/scenes/dropdown_demo.rs`. Grep `examples/` for others.
- **Vertical centring.** Wherever text is centred vertically with a literal `8.0`, use the line height: `let line = self.line_height(&TextStyle::default());`, then `y + (height - line) / 2.0`. The sites are `context.rs:1940`, `finish_dropdowns`'s option text, and any `self.font.char_height()` that survived Task 8. Grep for `8.0` near `draw_text` in `context.rs`.
- **Checkbox label, slider, toggle.** They already centre by `text_size`. Read each once against the new metrics: a 14 px font is about 17 px tall, against the old 8 px cell.
- **`wrap_text_styled(text, width, style)`.** The same as `wrap_text` with a style (the console needs its own font).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ryo2d --lib ui::immediate scene::manager`
Expected: pass.
Run: `cargo test -p demo-kit` and `cargo test --test e2e`
Expected: pass. `shell.rs` drives real dropdowns and text inputs; its assertions are about input, so they hold.

- [ ] **Step 6: Docs**

`src/core/ui/AGENTS.md`:
- `ui.text(..)` is the full text call (markup, wrap, anchor, reveal, fade, opacity); the `draw_text*` family is the shorthand with the default style.
- Open dropdown lists draw on the popup layer, flushed by the engine after each scene's `ui`.
- Layers order before submission.

- [ ] **Step 7: Gate and commit**

```bash
git add -A
git commit -m "feat: ui.text and widgets on real fonts" -m "- ui.text draws markup with wrapping, anchors, reveal, fade and opacity
- open dropdown lists draw on a popup layer the engine flushes after each scene's ui
- widgets centre text by the font's real line height"
```

### Task 10: Console, overlays, demo-kit chrome, dev windows

**Files:**
- Create: `src/core/text/cpu.rs`
- Modify:
  - `src/core/console/mod.rs`: JetBrains Mono, measured line height, wrapping through the text engine.
  - `src/core/config/debug.rs`: overlays measured and monospace.
  - `src/core/engine/frame.rs`: overlay and console sizes in UI units.
  - `src/core/windows/{dev_window.rs, logger_window.rs, profiler_window.rs}`: `CpuText` instead of `get_glyph`.
  - `examples/demo-kit/src/chrome.rs` (read and adjust only if the tour shows a problem).
  - `src/core/text/mod.rs`, `src/core/windows/AGENTS.md`.

**Interfaces:**
- Consumes: `UiContext::{text, measure_text, line_height, wrap_text_styled, draw_rect, ...}`, `TextStyle::role(FontRole::Mono)`, `Fonts::with_builtins`, `GlyphRasterizer`, `GlyphKey::fill`, `glyph_id`.
- Produces (crate-internal):
  - `CpuText::{mono(px: f32) -> Option<CpuText>, advance(&self) -> i32, line_height(&self) -> i32, measure(&self, text: &str) -> i32, draw(&mut self, buffer: &mut [u8], width: u32, height: u32, text: &str, x: i32, y: i32, color: [u8; 4])}`;
  - `console::CONSOLE_STYLE`, a fn returning the console's `TextStyle`: JetBrains Mono, 13 px.

- [ ] **Step 1: Write the failing tests**

`src/core/text/cpu.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;

  #[test]
  fn monospace_text_measures_by_its_advance() {
    let text = CpuText::mono(12.0).unwrap();
    assert_eq!(text.advance(), 7, "JetBrains Mono at 12 px advances 7 px, like the old font");
    assert_eq!(text.measure("abcd"), 28);
    assert_eq!(text.measure("\u{00E9}t\u{00E9}"), 21, "characters, not bytes");
  }

  #[test]
  fn drawing_blends_glyphs_into_the_buffer() {
    let mut text = CpuText::mono(12.0).unwrap();
    let (w, h) = (40u32, 16u32);
    let mut buffer = vec![0u8; (w * h * 4) as usize];
    text.draw(&mut buffer, w, h, "Hi", 1, 1, [255, 255, 255, 255]);
    assert!(buffer.chunks_exact(4).any(|p| p[0] > 200));
    text.draw(&mut buffer, w, h, "far off", 500, 500, [255, 0, 0, 255]);
  }
}
```

In `console/mod.rs`'s tests, add a wrap test that needs no GPU. It renders into a recording `UiContext`. The `context` and `text_engine` helpers are Task 8's; copy them into this module or make them a crate-internal `ui::test_util`, which is better since Tasks 8-10 all use them:

```rust
  #[cfg(feature = "default-fonts")]
  #[test]
  fn long_output_lines_wrap_to_the_console_width() {
    use crate::core::ui::immediate::{UiCommand, UiCommands};
    let mut console = ConsoleState::new();
    console.toggle();
    console.print("word ".repeat(80));
    let (mut commands, mut text) = (UiCommands::default(), crate::core::ui::test_util::text_engine());
    let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (1280.0, 720.0));
    console.render(&mut ui, 1280, 720);
    drop(ui);
    let (_, _, output_width, _) = console.output_area();
    let widths: Vec<f32> = commands
      .iter()
      .filter_map(|c| match c {
        UiCommand::Text { layout, .. } => Some(layout.size.x / layout.scale),
        _ => None,
      })
      .collect();
    assert!(widths.len() > 3, "the long line became several rows");
    assert!(widths.iter().all(|&w| w <= output_width + 0.5), "{widths:?} within {output_width}");
  }
```

`output_area()` returns `(x, y, width, height)` today (`console/mod.rs:375`); check the order. The test helper `context(commands, text, size)` takes the UI size, so make Task 8's helper take it.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib text::cpu console::`
Expected: compile errors.

- [ ] **Step 3: `CpuText`**

```rust
//! Monospace text for the dev windows' CPU pixel buffers (separate OS
//! windows that draw their own pixels), rasterized by swash from the
//! built-in mono font

use std::collections::HashMap;

use crate::core::text::{FontId, FontRole, Fonts, GlyphImage, GlyphKey, GlyphRasterizer, glyph_id};

pub(crate) struct CpuText {
  fonts: Fonts,
  font: FontId,
  px: f32,
  advance: i32,
  line_height: i32,
  ascent: i32,
  raster: GlyphRasterizer,
  glyphs: HashMap<char, Option<GlyphImage>>,
}
```

Implement:

- **`mono(px)`**
  - Set up the font: `let fonts = Fonts::with_builtins();`, `font = fonts.role(FontRole::Mono)?`.
  - Read the metrics with swash from the face's data (`FontRef::metrics(&[])` scaled by `px / units_per_em`):
    - `advance` is the advance of `'0'` rounded;
    - `ascent` is rounded;
    - `line_height` is `ascent + descent` rounded, but at least `px.round()`.
  - Return `None` without `default-fonts`.
  - If `advance` at 12 px is not 7 on this font, keep what the font says and update the test. `CHAR_WIDTH` and `CHAR_HEIGHT` become `CpuText`'s values (see Step 4).
- **`measure(text)`**: `text.chars().count() as i32 * advance`.
- **`draw(..)`**: for each character `c` at pen `(x + i * advance, y + ascent)`:
  - fetch or rasterize `GlyphKey::fill(font, glyph_id(c), px, 0.0)` into the cache (`None` for a space or a missing glyph);
  - blend each coverage texel `a` into the buffer with straight alpha: `dst = dst + (color - dst) * (a / 255) * (color.a / 255)`, clipped to the buffer;
  - skip pixels outside the buffer.

`GlyphImage` placement: the image's left edge is at `pen.x + left` and its top at `pen.y - top`.

- [ ] **Step 4: The dev windows**

- Replace `DevWindowRenderer::draw_text`'s body (dev_window.rs:455) with `self.text.draw(&mut self.pixel_buffer, self.width, self.height, text, x, y, color)`. `text: Option<CpuText>` is made once in the renderer's constructor; with `None`, nothing is drawn.
- `draw_text_shadowed` and `draw_tooltip` keep their logic.
- `measure_text` uses `CpuText::measure` (characters, not bytes).
- Delete `get_glyph` (dev_window.rs:616-780).
- `CHAR_WIDTH` and `CHAR_HEIGHT` stay `pub const`s at the font's measured values (7 and 12 expected), with a test pinning that `CpuText::mono(12.0)` matches them.
- The logger and profiler windows call `draw_text` and `measure_text` only. Check their 34 call sites compile unchanged and their layouts still read `CHAR_WIDTH`/`CHAR_HEIGHT`.

- [ ] **Step 5: The console**

In `console/mod.rs` `render`:
- **Style:** text uses `let style = console_style();`. That is `TextStyle::new().role(FontRole::Mono).size(13.0)`, with `.color(..)` per line, through `ui.draw_text_styled(..)`.
- **Line height:** `let line_height = ui.line_height(&style).ceil();` replaces the literal `12.0` in `render`, `handle_input`'s page scrolling (:500) and `output_area` (:375). Store the last measured line height in the state so `handle_input` can use it; it is measured each render.
- **Bars:** `TITLE_BAR_HEIGHT` and `INPUT_HEIGHT` become `line_height + 10.0` (computed). The constants stay as minimums.
- **Wrapping:** replace the console's own `wrap_text` (mod.rs:436-482, quadratic) with `ui.wrap_text_styled(line, width, &style)`.
- **Caret:** `ui.measure_text(&self.input_buffer[..self.cursor_pos], &style).x`.
- **Close button:** the "X" centred by `measure_text`.

Non-ASCII now draws in the console: typed text, and the em dashes the console demo prints.

- [ ] **Step 6: The overlays**

In `config/debug.rs`:
- **Style:** `render_fps_counter`, `render_frame_timing` and `render_stats` draw with the mono style (`TextStyle::new().role(FontRole::Mono).size(12.0)`), so `{:5}`-padded columns line up.
- **Pitch:** the row pitch is `ui.line_height(&style) + 6.0`, not 18.
- **Constants:** `FPS_COUNTER_HEIGHT`, `STATS_HEIGHT` and `FRAME_TIMING_HEIGHT` become functions of the line height, or are computed where used; keep the public names if anything outside uses them (grep).
- **Separator:** the `'\u{2500}'` row draws in JetBrains Mono, which the subset includes (Task 1's range).
- **`label`'s background:** it pads around the measured size, so the backgrounds fit the new font.

- [ ] **Step 7: The demo-kit chrome**

Run `./dev demos:tour` and look at three pages: one with settings, one with a long description, one with help open (`H`). The chrome derives its layout from `text_size("Ag")`, so it should adapt.

Fix only what the captures show is broken, for example a top bar too short for the new line height. Keep `tests/chrome.rs` green; if a threshold must move, explain why in the report.

- [ ] **Step 8: Run the tests**

Run: `cargo test -p ryo2d --lib text::cpu console:: config::` and `cargo test -p demo-kit`
Expected: pass.

Then run the logger window natively: `./dev profile-and-log` opens it. Open the console in a demo (`` ` ``) and look at both. Describe what you saw in the report: the dev windows cannot be captured headless.

- [ ] **Step 9: Docs**

- `src/core/windows/AGENTS.md`: the dev windows draw JetBrains Mono through `CpuText` (swash into their CPU buffers), measured by characters.
- `ryo-console`'s "Visual Style" section is updated in Task 13.

- [ ] **Step 10: Gate and commit**

```bash
git add -A
git commit -m "feat: console, overlays and dev windows on JetBrains Mono" -m "- the console wraps through the text engine and sizes its rows by the font
- overlay columns line up in a monospace font; the dev windows lose their hand-drawn font"
```

### Task 11: Examples ported and checked

**Files:**
- Modify, wherever the captures show a problem: `examples/**`. Known code changes:
  - `examples/games/flappy/src/scenes/menu.rs`: `draw_sprite(&Sprite)` becomes a `SpriteHandle`.
  - `examples/demos/asset-loading/main.rs`: `draw_sprite_handle_scaled` becomes `draw_sprite_scaled`.
  - `examples/demos/dpi-scaling/src/main.rs`: `ui_scale()` is `f32`; `request_ui_scale(Some(f32))`.
  - `examples/demos/ui/src/scenes/{dropdown_demo.rs, clipping_demo.rs, text_demo.rs}`.
  - `examples/demos/state-machine/main.rs` if its box-drawing lines need a style.

**Interfaces:**
- Consumes: everything from Tasks 1-10. The shorthand `draw_text*` signatures are unchanged, so most hooks compile as they are.
- Produces: no API; every example builds and looks right.

- [ ] **Step 1: Build everything**

Run `cargo build --workspace --all-targets`. Fix the compile errors; they should be only the known sites above:
- `draw_sprite` takes `&SpriteHandle`. In flappy's menu, keep a `SpriteHandle` made once (`SpriteHandle::from_sprite(sprite, "menu-bird")`, or the handle the game already loads), not a `&Sprite`.
- `draw_sprite_handle_scaled` is renamed `draw_sprite_scaled`.
- `ui.ui_scale()` returns `f32`, so drop the casts.
- `ctx.request_ui_scale(Some(n))` takes `Option<f32>`.
- `ui.finish_dropdowns()` is gone, which Task 9 already removed.

- [ ] **Step 2: Capture after, compare with before**

Capture with the same commands as Task 1 Step 1, into `target/ui-after/`. Then look at every pair (before and after) for:
- the five games;
- the `demo-tour` pages of: `ui` (every page), `ui-animations`, `tweening`, `trails`, `state-machine`, `controller`, `cursor`, `console`, `dpi-scaling`, `procgen`, `particles`, `networking`, `inventory`, `validated-animator`, and the demo-kit chrome on any page.

The new default text is 14 px Inter (about 17 px per line) where the old was an 8 px cell. Expect overlaps where an example spaced lines by a hand-picked pitch under about 18 px. For each problem, apply the smallest fix that keeps the example's design:
- a larger pitch;
- a smaller style through `draw_text_styled(.., &TextStyle::new().size(11.0))`;
- a wider box.

Do not redesign an example.

Record a table in the report: example, page or scene, what was wrong, fix. Include the examples that needed nothing.

- [ ] **Step 3: Non-ASCII text**

| Where | Text | What to do |
|---|---|---|
| `examples/demos/ui/src/scenes/dropdown_demo.rs:90-95` | `日本語`, `한국어`, `中文` | No built-in font has CJK, so they draw tofu. Replace them with `Ελληνικά`, `Русский`, `Čeština`, which the subsets cover, so the demo shows real non-ASCII text. |
| `examples/demos/state-machine/main.rs:488,533` | `━` (U+2501) | Falls back to JetBrains Mono (Task 2's stack ends with it), so it draws. If the capture shows otherwise, draw that text with `TextStyle::new().role(FontRole::Mono)`. |
| `examples/demos/state-machine/main.rs:559` | `▶` (U+25B6) | The same as the line above. |
| `examples/demos/console/main.rs` | em dashes | In the subset; they draw. |
| `examples/demos/trails/main.rs:840` | `35°` | In the subset; it draws. |

- [ ] **Step 4: Copy that describes the old limits**

- `clipping_demo.rs`: any text about a 16-deep limit or panics.
- `text_demo.rs`: the bitmap font. It becomes a short showcase of wrapping and centring with the real font; the full text demo is Task 12.
- `dpi-scaling`: its description of what `ui_scale` does. It now scales the whole UI, not just glyphs.

- [ ] **Step 5: Run the tests**

Run: `cargo test --workspace`
Expected: pass, including the bullet-hell and leap-king headless tests and demo-kit's.

- [ ] **Step 6: Gate and commit**

```bash
git add -A
git commit -m "fix: examples fit the new UI text" -m "- pitches and boxes sized for the real font; dropdown demo shows Greek, Cyrillic and Czech
- sprite UI calls take handles"
```

### Task 12: The `text` demo, with its web build

**Files:**
- Create: `examples/demos/text/{Cargo.toml, Ryo.toml, Trunk.toml, index.html, main.rs}`. Copy the `physics` demo's four config files, renaming `physics-demo` to `text-demo` and the titles to "Text".
- Modify: `Cargo.lock` (the new workspace member).

**Interfaces:**
- Consumes: `demo_kit::prelude::*` and the public text and UI APIs from Tasks 1-11.
- Produces: the demo.

The demo follows the demo-kit standard (`examples/demos/AGENTS.md`):
- arrows belong to the settings panel;
- pages keep their content left of x ≈ 520;
- it loads no files, so icons and nine-slice sheets are drawn into sprites in code (`Sprite::solid_rect` plus pixel writes, or `Sprite::from_rgba` if that exists; check `graphics/sprite.rs`).

- [ ] **Step 1: Write the demo**

```rust
//! Text: fonts and sizes, rich text with icons and markup, the typewriter and
//! glyph effects, world text, and GPU-drawn UI. Loads no files, so it runs on
//! the web too.

use demo_kit::prelude::*;

fn main() {
  Demo::new("Text")
    .resolution(960, 640)
    .page(FontsPage::default)
    .page(RichPage::default)
    .page(RevealPage::default)
    .page(WorldPage::default)
    .page(DrawingPage::default)
    .run();
}
```

The five pages. Every setting is a field read in `ui` or `update`.

1. **`FontsPage`** ("Fonts and sizes"):
   - **Content:** a column of the same sentence at 10, 12, 14, 18, 24, 32 and 48 px in the selected role's font, each prefixed with its size. Below them, the built-in fonts by name: Inter Regular, Inter Bold, JetBrains Mono, and m5x7 at 16 and 32 px.
   - **Settings:**
     - "Font": `Ui`, `Mono` or `Pixel`;
     - "UI scale": `Auto`, 1, 1.5, 2 or 3, through `ctx.request_ui_scale` (`Auto` is `None`);
     - "Weight": 400 or 700.
   - **Description:** "The built-in fonts at real pixel sizes; the UI scale makes the whole UI bigger, and text stays sharp at any display scale."
2. **`RichPage`** ("Rich text"):
   - **Content:** a paragraph from markup using every style tag: colour, size, font, bold, italic, outline. Below it, three lines with inline icons (`Press [icon=jump] to jump`, `[icon=attack] attacks`, `[icon=menu] opens the menu`).
   - **Icons:** 16×16 sprites drawn in code: a key cap with a letter shape, and a round gamepad button. They are registered in `TextIcons` under `jump`, `attack` and `menu`.
   - **Settings:** "Icons": `Keyboard` or `Gamepad`. It re-inserts the names with the other set, which shows that swapping sets needs no text change.
   - **Also:** a deliberately malformed line (`[color=red]never closed`) drawn as written, with a caption saying so.
3. **`RevealPage`** ("Typewriter and effects"):
   - **Content:**
     - A dialogue box, a rounded rect with a border, showing a three-sentence line with `ui.text(..).reveal(shown).fade(..)`, where `shown` advances by `speed × dt`. `Space` restarts it; a `[pause=0.6]` sits after the first sentence.
     - Under it, three lines in `[wave]`, `[shake]` and `[rainbow]`.
     - A custom effect, `bounce`, registered in `on_enter` with `ctx.world.resource_mut::<TextEffects>().register(..)`.
   - **Settings:** "Speed" (10-80 characters/s), "Fade" (0-4 characters), "Punctuation pause" (0-0.5 s).
   - **Controls:** `Space`, "Restart the line".
4. **`WorldPage`** ("World text"):
   - **Content:**
     - Three coloured sprites moving left and right, each with a child `Text2d` name tag above it.
     - A sign: a `Text2d` with an outline and a shadow.
     - Clicking or pressing `Enter` spawns a damage number at a random sprite. It is a `Text2d` with a random value. A tween raises it 40 units and fades it out over 0.8 s, then it despawns. Use the existing `Tween` API (`ryo-animation` skill) and a timer, or the tween's completion.
   - **Settings:**
     - "Zoom": 0.5, 1, 2, 4 or 8, via `ctx.camera().set_zoom`;
     - "MSDF": off or on. On calls `ctx.fonts().set_render(bold, FontRender::msdf())` on Inter Bold, which the tags and sign use (`TextStyle::bold()`), and shows sharp zooming and glow;
     - "Effects": none, outline, shadow, or glow (MSDF only; a caption says so when MSDF is off);
     - "Pixel font": off or on, which switches the tags to `FontRole::Pixel`.
   - **Controls:** `Enter` or the mouse: "Spawn a damage number".
5. **`DrawingPage`** ("UI drawing"):
   - **Content:**
     - A bright, colourful background: world sprites, so the UI's translucency shows over something.
     - Panels at 25%, 50% and 75% opacity, rounded rects with borders, and a nine-slice panel (a 24×24 sprite with 8 px corners, drawn in code) stretched to three sizes.
     - A line with corners (`draw_line`).
     - A scroll view whose content is a list of 30 rows of text, clipped by its scissor.
   - **Settings:** "Corner radius" (0-16), "Border" (0-4), "Panel opacity" (10-100%).

Write `main.rs` in full in the style of `examples/demos/physics/main.rs`: one struct per page with `Default`, and `impl DemoPage` with `name`, `description`, `settings`, `controls`, `on_enter`, `update` and `ui` as each page needs.

- [ ] **Step 2: Check it**

1. `cargo build -p text-demo` and `cargo clippy -p text-demo -- -D warnings`.
2. `./dev demos:tour text`, then look at all five PNGs (`target/demo-tour/text/page-*.png`) and describe each in the report.
3. Capture page 4 with `RYO_INPUT` pressing `Enter` a few times, so damage numbers show in a frame.
4. Run it windowed (`./dev text` or `cargo run -p text-demo`) and try each page's settings.
5. Build it for the web: `trunk build --release` in `examples/demos/text`.
   - Record the `.wasm` size from `dist/`, and the `character` demo's for comparison (build that too).
   - If the Playwright MCP browser tools are available to you, open it in Chromium, WebKit and Firefox and note console errors. If they are not, say so: the controller does it.
   - Delete `.playwright-mcp/` and `dist/` afterwards.

- [ ] **Step 3: Gate and commit**

```bash
git add -A
git commit -m "feat: text demo" -m "- fonts and sizes, rich text with icons and markup, typewriter and glyph effects
- world text with zoom, MSDF outlines and glow, pixel text and damage numbers
- UI drawing: translucent and rounded panels, nine-slice, lines and clipping"
```

### Task 13: Docs, skills and benchmarks

**Files:**
- Create: `.claude/skills/ryo-text/SKILL.md` and its byte-identical copy `src/cli/skills/ryo-text.md`
- Modify:
  - `src/cli/commands/install_skills.rs`: register `ryo-text` in `EMBEDDED_SKILLS`, beside `ryo-physics`.
  - `.claude/skills/{ryo-ui, ryo-api, ryo-console, ryo-engine-dev, ryo-renderer, ryo-game-dev}/SKILL.md` and their `src/cli/skills/` copies.
  - `src/core/text/AGENTS.md`, `src/core/ui/AGENTS.md`, `src/core/ui/immediate/AGENTS.md`, `src/core/render/AGENTS.md`, `src/core/graphics/AGENTS.md`, `src/core/AGENTS.md`, `src/core/windows/AGENTS.md`.
  - `docs/architecture/overview.md`.
  - `benches/text.rs`.
- Delete nothing. The plan is deleted at the finish, by the controller.

**Interfaces:**
- Consumes: the whole public surface from Tasks 1-12.
- Produces: docs and numbers.

- [ ] **Step 1: The `ryo-text` skill**

The frontmatter `description` is "Text: fonts, styles, rich text and markup, inline icons, typewriter reveal and glyph effects, world text (Text2d) and the damage-number recipe."

Sections:
1. **When to use what:**
   - `draw_text` and its family: the shorthand, default style;
   - `ui.text(..)`: every option in the UI;
   - `Text2d`: text in the world.
2. **Fonts:**
   - the built-ins and their roles;
   - `ctx.fonts()`: `load` (native) and `add_bytes(include_bytes!(..))` (web);
   - families, `find`, `set_role`;
   - `FontRender` (`Raster`, `Pixel`, `Msdf`) and when to choose each;
   - `prewarm`;
   - bitmap fonts (`add_bitmap`).
3. **`TextStyle`** (every field and its default) and **`TextLayoutOptions`**.
4. **`RichText`** (spans, icons, pauses) and the **markup** table: every tag, its value format, `[[`, the effect tags, and what malformed markup does.
5. **Icons:** `TextIcons`, and swapping sets.
6. **Effects:** the built-ins with their arguments; registering one with `GlyphContext` and `GlyphMod`; real time in the UI, game time in the world.
7. **Reveal:** in the UI (`reveal`/`fade`, advanced by the caller), and in the world (`TextReveal`, `TextRevealed`).
8. **World text:** `Text2d` (every builder), `TextBounds`, lighting, z, interpolation, per-camera sharpness under zoom.
9. **Recipes**, each a complete snippet:
   - a HUD with a score;
   - a dialogue box with the typewriter;
   - a sign with an outline;
   - damage numbers (`Text2d` plus tweens plus despawn);
   - pixel-art text;
   - a custom effect;
   - a game font loaded on native, embedded on the web.
10. **Limits:**
    - glow needs MSDF;
    - MSDF outline and glow widths are limited by the range;
    - m5x7 is ASCII-only;
    - CJK and emoji need a font that has them;
    - the binary-size cost of the built-in fonts and the `default-fonts` feature;
    - the web has no file system.

Check every recipe compiles against the prelude only. Extract each into a temporary test file under `tests/` that builds a `HeadlessEngine` scene, run it, then delete the file before committing. Say in the report how you checked.

- [ ] **Step 2: The other skills**

- **`ryo-ui`.** Replace the "Immediate Mode Drawing" text block (bitmap font) and the clipping and themes notes. Cover:
  - UI units and `ui_scale` (`Option<f32>`, automatic) and `UiResolution`;
  - the new primitives (`draw_rounded_rect`, `draw_rect_bordered`, `draw_nine_slice`, `draw_sprite` with handles, `draw_line`);
  - text through the shorthand family, or `ui.text(..)` (link `ryo-text`);
  - clips with no depth limit;
  - open dropdown lists on the popup layer, with no `finish_dropdowns`;
  - alpha blends in sRGB space ("a 50% black panel over white is mid grey, as on the web");
  - the text input editing any script.
- **`ryo-api`:**
  - the UI table (`ryo-api/SKILL.md:1030-1056`): remove `BitmapFont` and `create_debug_font`, add the new methods;
  - a new "Text" section listing every public text type with its signatures;
  - `EngineBuilder::ui_scale(Option<f32>)`, `ui_resolution(UiResolution)`, `SceneContext::request_ui_scale(Option<f32>)`, `SceneContext::fonts()`.
- **`ryo-console`:** "Visual Style" (JetBrains Mono, rows sized by the font, wrapping any script).
- **`ryo-engine-dev`:** the module table gains `text/`. The rendering diagram's UI line becomes "UI commands, then the UI camera through the batch, then the UI image, then the composite". The dev windows draw `CpuText` (no hand-made font).
- **`ryo-renderer`:** the `ui` pass, the UI image and its gamma-space composite, the text shader entries, the `UI_GAMMA` override.
- **`ryo-game-dev`:** any recipe drawing text, checked against the new defaults (no API change needed).

Copy each changed skill to its `src/cli/skills/` twin and `cmp` every pair.

- [ ] **Step 3: AGENTS files and the overview**

Read every AGENTS section Tasks 1-10 added and make each file read as one document:
- `text/` covers fonts, layout, markup, effects and reveal, glyph images, world text and `CpuText`;
- `ui/` and `ui/immediate/` cover commands, the UI image, scale, clips and popups;
- `render/` covers the pass table, the batch lanes, the UI camera and the frame steps;
- `graphics/` covers the glyph pages and the diagram;
- `src/core/AGENTS.md` gets `Fonts` in the resources list and the UI pass's commands;
- `windows/` covers `CpuText`.

In `docs/architecture/overview.md`, add a "Text and UI" paragraph next to rendering. It covers:
- the text core (Parley, swash and fdsm confined to `src/core/text/`);
- the glyph atlas;
- world text in the draw list;
- the UI as commands drawn by a UI camera into a gamma-space UI image.

- [ ] **Step 4: Benchmarks and sizes**

Add to `benches/text.rs`, beside the HUD frame:

```rust
/// 1,000 unchanging UI texts: every frame is layout-cache hits
struct StaticTexts;

impl Scene for StaticTexts {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    for i in 0..1000 {
      let x = (i % 20) as f32 * 32.0;
      let y = (i / 20) as f32 * 7.0;
      ui.draw_text_styled("label", x, y, &TextStyle::new().size(6.0));
    }
    SceneAction::Continue
  }
}

/// 200 world labels on moving entities
struct Labels;

impl Scene for Labels {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    for i in 0..200 {
      let (x, y) = ((i % 20) as f32 * 30.0 - 300.0, (i / 20) as f32 * 30.0 - 150.0);
      ctx.world.spawn((Transform::from_xy(x, y), Text2d::new(format!("#{i}"))));
    }
  }
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}
```

Bench both with the HUD bench's pattern: `"headless frame, 1000 cached UI texts"` and `"headless frame, 200 world text labels"`.

Then run `cargo bench --bench text -- --noplot` with nothing else CPU-heavy running. In the report, record:
- the HUD frame now, against Task 1's CPU-UI baseline;
- the two new benches;
- the per-glyph MSDF time from Task 4;
- the release size of `breakout` now, against Task 1's;
- the text demo's and the character demo's `.wasm` sizes from Task 12.

If the HUD frame is slower than the baseline, profile it before committing (`./dev breakout -p`, or a puffin scope around `emit_ui`), and report what dominates.

- [ ] **Step 5: Final checks**

1. `./dev demos:tour`: every demo, all pages; look at the ones touching text and UI.
2. The five games captured headless, checked by eye (Task 11's commands into `target/ui-final/`).
3. The gate.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "docs: text and the GPU UI" -m "- a ryo-text skill; ui, api, console, engine-dev and renderer skills updated
- text benchmarks: HUD, cached UI texts, world labels"
```
