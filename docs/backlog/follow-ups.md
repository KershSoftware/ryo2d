# Follow-ups

Small issues found while building each sub-project and left for later: deferred review findings, known limits and pre-existing bugs seen along the way. Each line names where it came from. Remove a line when it is fixed.

Items from 4f and 4g were copied from their execution records. Items from earlier sub-projects come from notes kept at the time, and some may have been fixed since: check before working on one.

## UI rendering (4g, UI render retention)

- A frame drawn while the glyph atlas refused glyphs (a sustained overflow, already warned) commits its UI image; a later frame with freed pages would draw the missing glyphs without retention. Fix: do not commit the image when `glyphs_refused` grew during `emit_ui`.
- Kept frames drop the UI camera's uniform buffer and bind group; the next changing frame recreates them.
- `UiRenderEnv.bitmaps` also keys the text cache, so a bitmap font sheet updated in place drops every cached text (one full rebuild per update; matters only for a sheet rewritten every frame).
- `add_bitmap` fixes the column count from the sheet width at registration; a sheet updated in place at another width keeps stale columns (pre-existing; same with retention on and off).
- Text drawn above its largest glyph image (world text above 256 px, UI text above 512 px) is scaled with linear filtering, so runs whose glyph atlases are packed differently can differ by one level at those glyphs' edges.
- The one-label changing frame misses the 60 µs target (81-96 µs): the rest is the batch's prepare, upload and encode (approach C).
- `UiRenderStats` is public without `#[doc(hidden)]` (as `UiCacheStats` from 4f).
- `UiTextCache::end` allocates a temporary `Vec` on frames that drop entries.
- Glyph atlas: `place()`'s post-eviction `allocate(..).unwrap_or(Placement::Full)` does not count `refused` (unreachable: a glyph smaller than a page fits a cleared page); evictions are counted outside the `if let` that clears the page.
- The ui_render guard's `saturating_sub` turns a below-empty idle median into 0 ns; its verdict strings use `to_string()`.
- The 4g backlog section does not explain why the profile's ~69 µs is below the guard's 81-96 µs.
- Tests: no one-field-mutation table test for `UiRetain::same` (Line arm, `Icon::Sprite` stamp, region-only icon change, clip/layer/tint/filter/reveal/opacity/seed, env fields, `clear`); the icon stamp is a 64-bit hash (a collision is a false keep; negligible); no test that an earlier frame's glyph-page pin does not block a later eviction; the `push_glyphs` test runs on an empty list and checks only `glyphs_from`'s length; the UI-only glyph-pressure test cannot catch the page-generation check (the world-churn test does); "font reload" is covered by `set_render`, not a reload; the alternating and pixel-art tests assert only `reused() > 0`; the sprite test does not assert a redraw after the size change; the pixel oracle compares the capture image, not `window_frame()`; the scrolling bench does not assert the list moved.
- Demo and game oracle tests: breakout's pause is not asserted reached; the ui demo's modal close is not asserted and its button is found by a hard-coded x (420.0); pointing and scrolling steps are not asserted per page; `PAGES = 17` is hard-coded.

## Debug overlays and console (4g hand-play)

- After toggling `debug:ui-rebuilds` off and on, the overlay shows the previous second's numbers for up to a second (the tally persists while hidden; reset it when the overlay is off).
- The overlay counter's unit test never crosses a second roll; a dt-0 first pass shows a blank counter; a doc comment wraps awkwardly (`config/debug.rs` ~236); no test of the overlay's drawn render line.
- `overlay_stack_top` duplicates the three engine overlays' placement formulas (a test catches drift); its test does not assert counter-above-line order or the `frame.rs` flag wiring.
- The stats and FPS overlay panels overlap by 8 px when stats, FPS and frame timing all show (pre-existing).
- `logical_to_screen`'s doc merged into `console_pixel_scale`'s (`engine/frame.rs` ~366); `logical_to_screen` is undocumented.
- A console scrollbar drag during new output overrides the unseen-lines adjustment; the 1,000-line cap with unseen lines is untested.
- The console's pointer ignores `mouse_scale` under an explicit `ui_scale`: its hit tests compare a screen-unit pointer with UI-unit rects (pre-existing).
- A game re-requesting a custom cursor that fails to create retries (and warns) every frame.
- The cursor block in `about_to_wait` could become an `Engine` method so a headless test covers its wiring; the release re-apply is checked only by hand on macOS.

## Input and scrolling (4g hand-play)

- The dev logger window reads winit's physical `PixelDelta` / 10, so it scrolls twice as fast on a Retina trackpad (pre-existing).
- On the web, browsers report mouse wheels as pixel deltas: a notch moves at once with no list look-ahead, so a node list can show blank leading rows for one frame per notch (`.overscan(n)` hides it).
- The node-list look-ahead does not decay while the offset moves (a slow tail keeps the largest step, bounded by a view); a 4:1 event cadence (240 Hz display, 60 Hz pad) still rebuilds a row per event; a step bigger than any before it has one uncovered frame.
- The `ScaleFactorChanged` path for physical scroll deltas is not covered by a headless test; a scale-factor change also applies to scroll events already queued.

## UI node cache (4f)

- The unchanged node settings frame has ~1.8 µs of headroom under its 35 µs target; the bench guard runs in every filtered `cargo bench --bench ui`.
- `check_tree` does random `get`s; there is no bench with a large embedded tree.
- Tests: no test of `refresh_skins` with a replaced theme; no test of the overlay's spot-check counter.
- `Fonts`' generation is a per-instance counter (a replaced `Fonts` resource restarts it); text with icons stays stale after an icon sprite is resized in place (both pre-existing).
- A bad `ui:cache` argument prints only a usage line; the ryo-console row and a module-notes line for `ui:cache` read awkwardly.

## UI (4a-4e)

- 4d: `reload_trees` makes a `QueryState` every frame (~427 ns); `TreeSource` has no build generation; the placeholder rule needs the same kind and name.
- 4b: a dialog that removes its own opener leaves no focus; `ui.popup` lists are not capped; `ui.input().scroll_delta()` reads zero over UI; centre-anchored columns jump when a line appears; the networking and asset-loading hints still say mouse-only; `UiHarness` rewinds frames that draw nothing (the engine doesn't); `context.rs` (~4.5k lines) and `input/mod.rs` (~2.8k lines) want splitting.
- 4b rulings the owner may revisit: the wheel is consumed whenever the pointer is over top-scene UI; dialogs are opaque; `ui` scene changes are ignored during transitions except Quit.
- 4a: colour is in the text layout cache key, so a hue animation lays text out again (fades go through opacity); `draw_text_anchored` and `label` lay out twice; the console re-wraps every line each frame; `.ttc` collections with several faces lose glyphs; binary size could be trimmed (parley features, ICU data, a second nalgebra from fdsm); the dev windows and IME preedit were never checked by eye natively.
- Real pixel-art projects still draw the engine's overlays at low resolution.
- Assets: a sprite density (@2x) option, so a high-resolution raster draws at its logical size without a scaled child entity.

## Physics (3a, 3b)

- 3b controller: the first-cast retry kicks a character falling past a shifted corner sideways (prototyped fix: accept only floor-like retried hits); the lift adds slope creep (prototyped fix: lift along up by sunk / (normal·up)); some older autostep misses; a 0.2 px seam lip stops runs; transient sinking when landing on fast lifts.
- 3b: the resting push-out costs 13-18% of a 100-character frame (it also carries floor detection); re-check `make_queryable` (`BroadPhaseBvh::set_aabb`) on any Rapier upgrade.
- 3a: game systems ordered with `.after(other)` can land inside `FixedSet::Physics`, where the write-back overwrites their position and velocity writes; the engine never calls `check_change_ticks` (change detection degrades after days); optional levers not taken: an opt-in CCD-off switch (~20 µs per 1,000 bodies), a `read_back` restructure (~5 µs).
- 3a minors: thin 1 px debug lines break at scaled resolutions; particle circle colliders approximate under shear; forces queued while paused accumulate; joint warnings once per entity; a remade joint re-anchors at current poses; waiting joints are rescanned each step; disabled colliders' contact pairs are one step stale; overlapping triggers can report the same collider twice; the physics `recheck` set grows while paused; `propagate_transforms` skips disabled entities.
- Never checked by eye: leap-king's lamp shadows (softness 0.4) and its networked level under the new physics.

## Renderer (2a-2c)

- 2a: device loss exits with Ok; a parented main camera converts in parent space.
- 2b: the polyline builder allocates per trail per frame; `mip_chain` copies level 0; trails ignore `BlendMode::Premultiplied`; `BindGroupCache` resets past 64 entries; `Line2D` with a negative end width draws inverted; screen-fixed parallax shakes.
- 2c: soft shadows fade with distance and keep a hard outline (true penumbrae wait for the shadow rewrite); the engine's light, ambient and atmosphere presets read warmer and darker; auto-exposure snaps on resize; ambient occlusion's edge bias sees light above 1; `VolumetricSettings` is not range-clamped; shadow colour is decoded per vertex; custom effect shaders get sRGB `Color` params.

## Engine-wide

- Scene systems run during transitions; nested scroll views (the outer one takes the wheel); the mouse wheel is read in every fixed step of a frame; the prelude glob also exposes the `propagate`, `interpolate` and `kernel` module names.
