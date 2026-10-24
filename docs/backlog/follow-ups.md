# Follow-ups

Small issues found while building each sub-project and left for later: deferred review findings, known limits and pre-existing bugs seen along the way. Each line names where it came from. Remove a line when it is fixed.

Items from 4f to 5a were copied from their execution records. Items from earlier sub-projects come from notes kept at the time, and some may have been fixed since: check before working on one.

## Asset registry (5a)

- Owner decisions (2026-10-24):
  - The reload-hitch target is met in 5a, through two changes: images are uploaded over frames (14b), and shaders are compiled off the main thread (14c).
  - The determinism fix was made in 5a: draw-order ties and physics registration follow entity index (`ecs::entity_order`). The tour's baseline is Task 1's commit with that fix.
- Rulings the owner may revisit:
  - **R27:** a large image's first upload borders on the main thread, about 4 ms at 2048². Only reloads are prepared off the main thread.
  - **R29:** an `Assets<T>` store comes only from a world, with no public constructor. Tests use `testing::asset_world()`.
  - **R32:** an image's texture is never dropped for going undrawn. GPU memory is bounded by count, not bytes: 64 standalone textures (up to about 2.9 GB at 4K) plus 8 atlas pages. A full atlas clears its least recently used page.
  - **R33:** a `.wgsl` file must define `fn sprite` or `fn effect` to load as a `Shader`.
- Limits:
  - **Large first loads.** A large image first loaded with `load` during play uploads in the frame it lands (2–4 ms at 2048²). A shader first drawn during play, or a blend mode first used, compiles on the main thread in that frame and draws plain for one frame.
  - **Resized reloads.** A large image reloaded at another size is drawn in the UI at its natural size from the landing frame, so its old pixels are stretched for the spread's frames. If a `Modified` is missed (frames not drawn), the spread starts from the next draw, which keeps the 17 MB bordered copy until then. `MessageCursor::missed_messages` could make that fallback exact.
  - **Spreads.** They are served lowest key first, with no fairness across several large reloads. Frames during a shader compile cost about +0.1 ms. An engine runs up to three shader threads.
  - **Mip chains.** Large images keep their mips on the CPU for life (+5.7 MB at 2048²), so an evicted texture re-uploads without remaking them.
  - **Paths.** Asset keys are case-sensitive, so on a case-insensitive file system a path typed in another case loads but never hot-reloads. Theme sound paths, audio and `load_tiled_map` resolve against the working directory, not the asset root, until 5b and the tilemap work.
  - **Embedding.** An added asset has no path, so an embedded UI file cannot be imported by another; on the web, embedded imports need a `MemorySource`. `Theme::parse` is crate-only.
  - **Watching.** A folder created after startup is never watched. notify's dropped and rescan events are ignored. `Pending::record` is O(n²) for a large burst.
  - **Events.** A failed asset, or a load dropped before it lands, is freed with no `Removed`. A missing image warns again in every scene that asks for it (bullet-hell's backdrop gives 4 lines). A `NotFound` fallback on native warns once per file.
  - **UI files.**
    - A file that imports a cycle it is not part of reports only the cycle, not its own position.
    - A cached `parsed_check` error is permanent for that parsed document; key it on the imports' newest version.
    - Two brace cases are skipped at check time and load at build: a literal `{` inside a template body, and a brace path passed from the root as a template argument.
    - `check-ui` lists image issues after build issues, and files outside `assets/` fail as "not an asset path".
  - **Uploads.** Scenes have no query for a pending upload; only `HeadlessEngine::image_upload_pending` exists. A reloaded image that is not drawn keeps its old pixel `Arc` in the texture registry until it is drawn again or freed (check after 14b).
  - **Sibling query order.** It still follows query or command order, invisible in the tour, in composite tiles, world text, parallax, trails and lines, `push_changes`/`move_characters`, `track_disabled` re-enables and `forget_removed`.
- Pre-existing, seen during 5a:
  - Flappy's restart menu ignores Enter during its fade-in after Game Over, and START loses its focus ring (on both trees).
  - The `dev_window` test needs a `target/` folder under the checkout, so it fails with an outside `CARGO_TARGET_DIR`.
  - `Image::from_rgba` panics on a wrong length.
  - The text engine rebuilds Parley's font collection on every font add or free.
  - On wasm, the inline pump can stall a frame on one large decode beyond its 4 ms budget.
- Speed:
  - `next_settled` and `find_cycle` grow with the number of waiting values: about 6 ns per value per pump (measured in Task 14).
  - `Assets::len`/`is_empty` are O(slots).
  - Every `use_game_look` relays the UI text twice, once when the font copy is added and once when the old copy is freed; this happens once per menu entry.
- Code:
  - `AssetChanges` and `CheckContext` have no `Debug`.
  - `AssetError::is_missing()` was suggested and not added.
  - `AssetLoader::dependency_failed`'s doc does not say that the returned error must still describe the dependency failure.
  - A load of an unregistered type warns on every call.
  - `mod atlas` is `pub(crate)` only for the loader test's reach.
  - In `shaders/cache.rs:39` the field `made` is named for its origin, not its meaning.
  - Pipeline labels keep the first shader's name after that shader is freed.
  - A shader in both effect stages logs its compile error twice per version.
  - The renderer's fallback warning says "could not load" for a GPU rejection too.
  - Comment wrap at `sprite_gpu.rs` around line 861.
  - A doc comment in `benches/assets.rs` names `use_a` where the parameter is `import_a` (check).
- Tests:
  - A reversed edge pair changed together can fail the first-arriving reload with `Cycle` against the other's old edge, and nothing retries it; saving the file again recovers.
  - `watching_stops_when_the_source_drops` cannot tell a joined thread from a stopping one.
  - The first half of the N3 test (nothing drawn in the pass that loads) has no RED of its own.
  - The lib reload test (`testing/gpu_world.rs`) polls up to 2 s, so the 120-frame sweep can rescue it.
  - "Held but not drawn keeps its pipelines" is pinned only at the cache level.
  - Nothing pins the full check of an importer reloaded on the workers.
  - The shader panic test does not follow with a later save.
  - Breakout's quiet-fallback assertion reads the process-wide dev log.
  - `reload_and_wait` calls `AssetsView::new(world)` before its loop and panics without a registry.
  - `benches/assets.rs`'s default results dir ignores a cargo `build.target-dir` config.
- Examples:
  - no shipped example shows the debug checker for a renamed file, and none uses a `BitmapFont`;
  - the asset-loading Hot reload page picks up a file missing when it opened, but a `ui/card.ui.ron` that appears later loads without its panel being spawned (only `enter` spawns it).

## UI fragments (4i)

- Performance accepted as measured (owner decision, 2026-10-22; numbers and levers in `docs/backlog/ui-retained-nodes.md`, "4i: fragments (measured)"):
  - the UI pass while scrolling is 22.5 µs and the render step +31.3 µs, against the spec's 20 µs (guarded at 27 and 35 µs);
  - the virtual lists are 3 to 4% and scrolling with retention off 4.7% over before 4i;
  - the one-label render step meets its 25 µs narrowly and depends on load.
  - Levers measured and not taken: records placed by their fragment (still screens faster, focus and scrolling slower), a thin kept root (no gain), position-independent reuse of in-place runs (about 0.5 µs), padded stable runs (about 2 µs; interest-step frames, about one in five while scrolling, rewrite the instance array from the first row to leave), and R8 changed.
- Never run outside Metal:
  - the GPU suites on D3D12, Vulkan and GL/WebGL2, covering the slot uniform in the instance buffer (`VERTEX | UNIFORM`, dynamic offset), `write_buffer_with`'s joined write and the CPU-against-shader exact-pixel test. If that test flakes on another backend, make it a rigidity assertion;
  - the web build's slot uniform and vertex culling.
- Limits:
  - a window resize rebuilds every node (`UiEnv.size` is an input);
  - the pixel-art target never copies slot runs and places slot content on the CPU (R1), so at fractional scales its scrolled text can sit a pixel from the shader path's (R8);
  - the Big list at 1.25 rests with one row border a pixel off the pre-4i image after a wheel scroll (the half-pixel case);
  - a node list's rows trail a dragged thumb by one frame, and a track press pages it a frame later;
  - a game changing a node list's scroll axes or bar mid-drag gets the old ones for a frame.
- Rulings the owner may revisit:
  - R7 narrows plan difference 3: in debug builds a spot capture equal to last frame's fragment survives `restart_pass`, proven equal;
  - the spec puts an `at` node in "the area's parent slot"; the code keeps it in slot 0, as before 4i.
- Pre-existing:
  - `tests/e2e/ui_retain.rs`'s `glyph_pressure_from_the_ui_matches` flakes (main failed 3 of 85 runs). Retention on and off differ by pixels under UI glyph pressure, likely the scaled-glyph atlas limit (unverified);
  - release builds ignore `RYO_INPUT` and `RYO_HEADLESS`, and `RYO_INPUT` has no trackpad event.
- Speed:
  - `Scratch::capture_of` is a linear search, so a frame capturing C fragments costs O(C²): about 1 ms, once, for 1,000;
  - new runs can spend the held-run far-search budget early in a hold, leaving a later departure of more than 16 runs rescanned;
  - animated runs are stored with empty instances and always rejected, and empty runs force a redraw when their version changes;
  - `vs_ui` evaluates `quad_corner` five times per vertex;
  - `kept_column_frame` warms its two harnesses before its filter check.
- Code:
  - direct indexing, valid by construction, in `tree.rs` (resolve, `Placing`), `layout/engine.rs:237-238` and `ui_retain/mod.rs`;
  - `Inputs::slot` is a `u8` with magic values 0, 1 and 2;
  - `push_kept` returns false after side effects (unreachable);
  - `draw_clean` assumes a single-root tape, so a builder pushing a second top-level node would be dropped; this wants a debug assertion;
  - `outside_clip` duplicates `outside`;
  - two functions are named `spot_check`;
  - "slot" means both atlas slot and scroll slot in `fix_sprites`;
  - `copy_run` adds offsets unchecked while `keep_built` saturates;
  - an unneeded `too_many_arguments` allow sits on `command_of`;
  - `Placing::s` is set in `clear` while `place` reads `from.s` (equal by construction);
  - a slot index missing from the slot table marks its run absolute (degenerate, safe);
  - a record clip under an inner clipper in a slot is an ulp off at 1.25 (derive it by the absolute rule);
  - a spot-built capture that differs with equal inputs silently takes a new version instead of counting as a difference;
  - `choose_spot` loses fragment-check frames to the node spot path;
  - `rewind_frame` loses fragments taken in a rewound frame (harness only);
  - a lost fragment's slot persists across a rerun pass until the next frame.
- Console and docs:
  - the overlay prints copies as "kept";
  - `ui:fragments` says "on" while `ui:cache` is off;
  - the ryo-ui skill's "one and a half times a still frame" is the UI pass only, and its spot-check wording ("lay out afresh") is loose.
- Tests, weak or missing pins:
  - `a_page_at_rest_draws_as_it_did` checks only that rects sit on the grid;
  - the drawn offset's rounding mode;
  - `push_slot` saturation;
  - `a_kept_text_leaf`'s height check (the `asked` check is the one that discriminates);
  - a kept descendant without an entry, and `placed_below` on an unheld id;
  - a popup in `a_popup_or_a_placed_node_is_never_kept`;
  - `assert_reused` passes on any frame;
  - the Inputs checks that no scenario fails alone: scale, layer, scene, `in_popup`, the carried-only rule and the top flag;
  - a sprite's draw inside a run;
  - the CPU slot tests' coverage of own clips;
  - a cyclic or over-depth scroll chain in `Nav`'s `seen_from`;
  - the breakout HUD changing;
  - the empty-pass test asserts only `>`, and its rerun `+=` path is untested.
- Tests, shape:
  - the CPU slot test rebuilds its instance itself;
  - `scene_pushed_over_fragments` cannot split its stats per scene;
  - `a_window_resize` duplicates `each_env_change`;
  - the e2e runs snapshot is parsed from a debug string;
  - the hover test's `.take(8)` could hover fewer rows;
  - breakout's `game_folder` is copied a third time;
  - `an_evicted_glyph_page` runs 600 frames of readback;
  - the GPU build tests skip silently without a GPU;
  - `ui_renderer/gpu_tests.rs` passes 3,800 lines.

## UI renderer (4h)

- Analytic, derivative-free line anti-aliasing for UI and world lines (owner decision, 2026-10-20): its own small sub-project with a visual review; see `docs/backlog/ui-retained-nodes.md`. Until then UI line pixels on Apple GPUs differ from the pre-4h path only by the indexed-mesh `fwidth` (text demo page 5: 34 px at miter tips, at most 67/255).
- A moved text whose x changes by a fraction of a pixel is rebuilt (exact; vertical and whole-pixel moves reuse the cache).
- `GlyphSeed` is about 152 B and keeps a full `SpriteInstance` whose transform rows are always identity; a slimmer seed would cut the cache's memory.
- `plan_runs` allocates a fresh `Vec<Run>` every drawn frame.
- Out-of-order UI commands build glyphs in painter's order, not recorded order, so glyph atlas placement can differ from the pre-4h path (pixels unaffected except the known scaled-glyph limit above 512 px).
- `TexKey` duplicates `SlotPage` and makes `graphics` import `render::ui_renderer` (`runs.rs`, `sprite_gpu.rs`).
- `sprite.wgsl`'s "Sampling and colour space" header now covers only `texel_size`.
- Game sprite shaders get `quad_corner` in their prelude (`include/quad.wgsl`), so a game shader function of that name clashes.
- `SPRITE_LINEAR` (a render target shown as a UI sprite) is covered at GPU level only: no UI command can show a render target today.
- Tests: the CPU-only build checks are GPU-gated, so they pass vacuously without a GPU (the crate's pattern); the 3,000-label growth test relies on validation rather than asserting growth; no GPU test passes a kept or all-culled frame between uploads (safe by reasoning); the building tests compare `build_ui` against the shared functions it calls (their value is the fixed clips, kinds, order, culling and texture needs); no "state happened" asserts in the e2e `scroll_clipping` (scrolled), `sprites_and_a_standalone_image` (standalone) and `resize_and_theme_change` (applied) tests; the line comparison's mask admits any in-panel pixel that differs from the background (bounded by the line slack); `INTERLEAVED_RUNS == 4` depends on Inter's "◯" metrics and the page sizes without asserting the page counts; the pixel-art UI test's line is whole-pixel and horizontal; `lines_at` (e2e) and `test_lines` (GPU tests) are near-duplicate scenes; wgpu has no public strong count, so the release test checks only the renderer's own references to the textures it dropped.

## UI focus and debug overlay (4h hand-play)

- On the ui demo's Clipping & Modals page, the arrow keys move the focus between the horizontal cards to a different card from run to run (Card 2 or Card 3, about half the runs each, at UI scale 1.25 and after resizes), so the strip scrolls differently; directional focus seems to break a tie in an unordered way (pre-existing).
- The `debug:ui-rebuilds` overlay outlines a node list's pooled rows outside the list's clip, over the "Node list" heading and the legend on the Big list page (pre-existing).

## UI rendering (4g, UI render retention)

- A frame drawn while the glyph atlas refused glyphs (a sustained overflow, already warned) commits its UI image; a later frame with freed pages would draw the missing glyphs without retention. Fix: do not commit the image when `glyphs_refused` grew while the UI was built (`UiRenderer::build`).
- Kept frames drop the UI camera's uniform buffer and bind group; the next changing frame recreates them.
- `UiRenderEnv.bitmaps` also keys the text cache, so a bitmap font sheet updated in place drops every cached text (one full rebuild per update; matters only for a sheet rewritten every frame).
- `add_bitmap` fixes the column count from the sheet width at registration; a sheet updated in place at another width keeps stale columns (pre-existing; same with retention on and off).
- Text drawn above its largest glyph image (world text above 256 px, UI text above 512 px) is scaled with linear filtering, so runs whose glyph atlases are packed differently can differ by one level at those glyphs' edges.
- `UiRenderStats` is public without `#[doc(hidden)]` (as `UiCacheStats` from 4f).
- `UiTextCache::end` allocates a temporary `Vec` on frames that drop entries.
- Glyph atlas: `place()`'s post-eviction `allocate(..).unwrap_or(Placement::Full)` does not count `refused` (unreachable: a glyph smaller than a page fits a cleared page); evictions are counted outside the `if let` that clears the page.
- The 4g backlog section does not explain why the profile's ~69 µs is below the guard's 81-96 µs.
- Tests: no one-field-mutation table test for `UiRetain::same` (Line arm, `Icon::Sprite` stamp, region-only icon change, clip/layer/tint/filter/reveal/opacity/seed, env fields, `clear`); the icon stamp is a 64-bit hash (a collision is a false keep; negligible); no test that an earlier frame's glyph-page pin does not block a later eviction; the UI-only glyph-pressure test cannot catch the page-generation check (the world-churn test does); "font reload" is covered by `set_render`, not a reload; the alternating and pixel-art tests assert only `reused() > 0`; the sprite test does not assert a redraw after the size change; the pixel oracle compares the capture image, not `window_frame()`.
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
