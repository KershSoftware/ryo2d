# AGENTS.md - UI Context, Commands and Input

`UiContext` (`context.rs`) is what `Scene::ui` draws with. It draws nothing itself: every primitive records a `UiCommand` (`commands.rs`) that the engine's UI pass later lays into the 2D batch (`render/ui.rs`; the pass and the UI image are in `src/core/ui/AGENTS.md` and `src/core/render/AGENTS.md`). `input.rs` is the `UiInput` snapshot.

## Commands

- `UiCommand` is `Rect` (fill, radius, border), `Sprite` (a sprite handle, a rect, UVs, a tint), `Line` (a polyline, width, colour) or `Text` (a cached `TextLayout`, origin, reveal, opacity, effect seed). All are in UI units, and each carries the clip it was drawn under and its draw layer.
- Nine-slices are nine `Sprite` commands. Borders and rounded corners are the `Rect` command's, drawn analytically by the shader.
- `UiCommands` is cleared at the start of each UI pass, so a pass that runs twice in a frame (a scene switch returned from `ui`) keeps only the second pass's commands.
- Each command is stored with its `order`, taken when it was recorded (`next_order`). Absolute draws (`draw_*`, `ui.text(..).at(..)`) are pushed at once; a container's draws are pushed when it is laid out (`src/core/ui/AGENTS.md`, Layout and ids), so the list is not in painter's order: the order is.
- Rects, sprites and text wholly outside the current clip are not recorded; so are non-finite or empty rects.

## Clipping Stack

- `push_clip()`/`pop_clip()` nest without a depth limit; nested clips intersect with their parents. Each recorded command carries the clip it was drawn under, drawn as a scissor.
- `pop_clip()` on an empty stack does nothing and is warned once. Always pair every `push_clip` with a `pop_clip`.

```rust
ui.push_clip(outer);  // depth 1
ui.push_clip(inner);  // depth 2, intersected with outer
ui.pop_clip();        // depth 1
ui.pop_clip();        // depth 0 (no clipping)
```

## Text

- `ui.text(..)` (`UiText`) is the full call: markup, wrap, align, max lines, ellipsis, anchor, reveal, fade, typewriter and opacity; `draw()` returns the size in UI units, and a `UiText` dropped without `draw()` draws then. With `.at(x, y)` it is drawn there; without it, it is a layout leaf (wrapped to its width in a container, placed by its anchor outside one). The `draw_text*` family and `label` are the shorthand with the default style (`FontRole::Ui`, 14 px).
- Text is measured, wrapped and laid out through the text engine at the UI scale (`px_scale` render pixels per UI unit); layouts are cached, so an unchanged call is a hash lookup.
- `draw_text_centered` measures in the style it draws in, so the draw is a cache hit.
- **Fades go through opacity.** The layout cache keys on the style, colour included, so changing a style's colour every frame lays the text out again every frame. The shorthand helpers (`draw_text`, `label`, the wrapped and centred variants) lay the text out opaque and carry the colour's alpha as `UiCommand::Text { opacity }`, so one cached layout serves every alpha. `draw_text_styled` lays its style out as given (moving that colour's alpha to opacity would fade the style's outline and shadow too).
- **Typewriter.** A reveal with a fade wider than one cluster must be driven past the cluster count (`TextReveal::drawn_clusters`), or the last characters stay partly faded. `typewriter(&mut TextReveal)` does that for you: `draw()` advances the reveal by the UI's real `dt` over the text's own layout (so `[pause]` and punctuation pauses hold it) and records its drawn clusters and fade. One call a frame per reveal; no `TextRevealed` message for UI text (`is_done` says when it finished). `TextReveal::finish` (a dialogue's Skip) shows the rest at the next draw, its fade ended too.
- A text's effect seed is a hash of the text, the style without its colours and the options, so it does not move when other commands are drawn before it or when a hover changes a colour.
- The text field's (`widgets/text_input.rs`) click, caret, selection and scroll positions come from the engine (`UiContext::byte_at`, `caret_x`, `selection_x`), never a fixed character width, so it edits any script the fonts have. Vertical centring uses the font's real line height (`line_height`), never a fixed cell.

## Popups and Layers

- Every command has a draw layer; layers order before the recorded order (`ui_z`, which clamps layers to 127). The scene's page is `PAGE_LAYER` (0).
- Popups (open dropdown lists, `ui.popup`, modals), tooltips and the drag ghost record on `POPUP_LAYER` (1): `end_scene`, which `SceneManager::ui_pass` runs after each scene's `ui`, lays the scene's popups out after its page (`src/core/ui/AGENTS.md`, Popups and dialogs), so a scene never flushes them.
- Only the top scene's popups use layer 1. A lower scene's stay on layer 0, right after its own page, so the scenes above still cover them. The overlays and the console draw after the scenes on `OVERLAY_LAYER` (2), above every popup: `Engine::draw_ui` switches to it with the crate-internal `UiContext::set_layer`.

## Sprites and Shapes

- `draw_sprite*`, `draw_nine_slice` take `&SpriteHandle`s. UI sprites use linear filtering in the default presentation. Nine-slice insets clamp to the texture, and the corners shrink with a rect smaller than their sum.
- `draw_line` records one polyline command, drawn as a mitered strip with butt caps by the batch's polyline builder.
- `draw_modal_overlay` dims the whole UI area, ignoring the current clip (`with_no_clip`).
