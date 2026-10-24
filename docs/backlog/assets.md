# Asset Registry (Sub-project 5a)

Spec: `docs/specs/2026-10-23-asset-registry-design.md`.

## 5a: measured

Measured on an Apple M5 Pro (Metal), on a shared machine (a browser, Discord and an editor open). Medians move with the machine's load, so every run's 1-minute load is given.

**Method.** The build before the registry (2ba51866: the old `AssetManager`, with its own `benches/assets.rs`) and the branch ran in four alternated rounds; the side that ran first flipped each round. Each run waited until the 1-minute load was 4 or less and no other process was drawing on the GPU. Loads at the runs' starts were 1.2 to 3.9.
- Each side ran its own `benches/assets.rs`, `benches/ui_render.rs` and `benches/foundation.rs`. The last two are the same on both sides, but for the image API.
- The numbers are criterion's medians, and the guards' medians from their own output.

### The targets

| Target | Before | After | Verdict |
|---|---|---|---|
| Scene start, `load_now`: 40 images and 3 UI files | 6,096 µs (rounds 5,867, 6,131, 6,064, 6,127) | 5,470 µs (5,426, 5,507, 5,485, 5,454) | no slower: **met**, -10.3% (rounds -7.5, -10.2, -9.5, -11.0%) |
| Scene start, `load` and the workers, the same assets | 6,096 µs (synchronous) | 3,105 µs (3,169, 3,106, 3,104, 3,100) | faster: **met**, -49.1% |
| Empty frame, render step (`ui_render` guard) | 61.5 µs (rounds 61.2 to 62.0) | 63.1 µs (62.7 to 63.6) | within 3%: **met**, +2.6% |
| Idle settings screen, render step (`ui_render` guard) | 61.2 µs (59.3 to 62.3) | 62.6 µs (61.6 to 63.9) | within 3%: **met**, +2.3% |
| Empty frame, whole step (`ui_render`'s `empty_frame`) | 185.2 µs | 187.3 µs | within 3%: **met**, +1.2% |
| Idle settings screen, whole step (`settings_idle`) | 183.7 µs | 184.6 µs | within 3%: **met**, +0.5% |
| 10,000 sprites, whole step (`foundation`) | 1,059.6 µs | 1,006.3 µs | within 3%: **met**, -5.0% |
| Reload hitch: a 2048x2048 PNG | +4.0 to +4.2 ms, written in the frame it lands | +313 to +356 µs, the slowest frame of each reload until it is drawn (12 frames) | within 0.5 ms: **met** |
| Reload hitch: a 256 px SVG | +419 to +499 µs, made and written in the frame it lands | +150 to +184 µs, written in the frame it lands | within 0.5 ms: **met** |
| Shader reload: a sprite shader saved as new WGSL | +90.8 ms, compiled in the frame it lands, which draws the sprite plain | +104 to +136 µs, the slowest frame of each reload until it is drawn (7 frames), the old shader drawing meanwhile | within 0.5 ms: **met** |
| Shader reload: a display effect, and an HDR effect | +72.5 and +71.8 ms, compiled in the frame it lands, which skips the effect | +81 to +104 and +80 to +99 µs (6 frames), the old effect running meanwhile | within 0.5 ms: **met** |

The fonts (`scene_start_fonts`, branch only: the build before had no public way to make a `Fonts` registry) take the same set to 5,505 µs (5,469, 5,511, 5,541, 5,499), 4 to 55 µs over the round's `scene_start`. `scene_start_async`'s workers have already started (an untimed one-pixel load), as a game's have by its second scene; starting them was not measured. The workers make each small image's upload (its bordered pixels and the pages' levels) as they load it, about 1.4 ms of the 3.1 ms of worker time: the frame that first draws those images then makes none of it.

### Scene start (`benches/assets.rs`)

| Bench | Round 1 | Round 2 | Round 3 | Round 4 |
|---|---|---|---|---|
| before, `scene_start` | 5,867 µs (load 1.18) | 6,131 µs (2.60) | 6,064 µs (2.94) | 6,127 µs (2.89) |
| after, `scene_start` | 5,426 µs (1.23) | 5,507 µs (2.35) | 5,485 µs (2.72) | 5,454 µs (2.07) |
| after, `scene_start_fonts` | 5,469 µs | 5,511 µs | 5,541 µs | 5,499 µs |
| after, `scene_start_async` | 3,169 µs | 3,106 µs | 3,104 µs | 3,100 µs |

### Idle and empty frames (`benches/ui_render.rs` and `benches/foundation.rs`, four rounds)

The `ui_render` guard reads the render step: seven engines stepped in turn, with the GPU idle before each step. The values are medians of the four rounds, with the range of the rounds.

| Frame | Before | After | Change |
|---|---|---|---|
| empty frame, render step | 61.5 µs (61.2 to 62.0) | 63.1 µs (62.7 to 63.6) | +2.6% |
| idle screen, render step | 61.2 µs (59.3 to 62.3) | 62.6 µs (61.6 to 63.9) | +2.3% |
| one label, over the empty frame | +18.7 to +22.1 µs | +20.2 to +22.8 µs | |
| scrolling, over the empty frame | +22.5 to +26.9 µs | +24.3 to +27.4 µs | |

Both builds met all of the guard's own targets in every round. At loads near 4 they miss their one-label (25 µs) and scrolling (35 µs) targets by a few µs, both alike; those are not the registry's targets.

Criterion's medians of the same four rounds:

| Bench | Before | After | Change |
|---|---|---|---|
| `empty_frame` | 185.2 µs | 187.3 µs | +1.2% |
| `settings_idle` | 183.7 µs | 184.6 µs | +0.5% |
| `settings_idle_unretained` | 178.6 µs | 178.1 µs | -0.3% |
| `settings_label` | 127.4 µs | 127.7 µs | +0.2% |
| `settings_scrolling` | 134.5 µs | 135.2 µs | +0.5% |
| headless frame, 10k sprites | 1,059.6 µs | 1,006.3 µs | -5.0% |
| propagate 10k, every root moved | 73.5 µs | 73.7 µs | +0.3% |
| propagate 10k, nothing moved | 7.5 µs (6.9 to 8.5) | 8.1 µs (6.8 to 9.4) | +7.5% |

Stepped back to back, `settings_label` and `settings_scrolling` are paced by the GPU: on a busier machine each reads one of two values a run on both builds (`settings_scrolling` about 142 or about 225 µs). On this quiet machine both read steadily.

The 10k-sprite frame looks up each sprite's place in the atlas (`TextureAtlas::lookup` and `texture`, through `SpriteGpuRenderer::slot`). Once the atlas grew, those calls stopped being inlined, and the frame took about 40 µs (3.8%) longer than with the build just before. Marked `#[inline]`, the frame is faster than that build's.

Propagation ran the same source on both sides. Its 8 µs "nothing moved" case swings from 6.8 to 9.4 µs between rounds of one build, so its change is noise, not the registry.

**After the shader reload work**, the targets above were measured again in a second campaign, four alternated rounds against 2ba51866 (loads 3.5 to 4.0 at the runs' starts). The machine was busier than in the first (a music application and the window server took about 90% of a core between them), so both sides read higher than in the tables above; the changes are what count.

| Target | Before | After | Change |
|---|---|---|---|
| Scene start, `load_now` | 6,339 µs (6,340, 6,291, 6,364, 6,338) | 5,911 µs (5,909, 5,912, 5,892, 6,040) | -6.8%: **met** |
| Scene start, `load` and the workers | 6,339 µs (synchronous) | 3,234 µs (3,269, 3,228, 3,239, 3,223) | -49.0%: **met** |
| Empty frame, render step (`ui_render` guard) | 71.8 µs (70.4, 71.6, 75.5, 72.0) | 71.4 µs (73.0, 72.1, 70.6, 70.5) | -0.6%: **met** |
| Idle settings screen, render step | 72.0 µs (71.1, 71.3, 76.1, 72.6) | 71.0 µs (73.2, 71.8, 69.9, 70.2) | -1.3%: **met** |
| `empty_frame` | 208.5 µs | 201.1 µs | -3.5%: **met** |
| `settings_idle` | 211.5 µs | 201.8 µs | -4.6%: **met** |
| 10,000 sprites (`foundation`) | 1,107.0 µs | 1,075.1 µs | -2.9%: **met** |

`scene_start_fonts`: 5,945 µs. The reload guard read the PNG at +332 to +356 µs and the SVG at +121 to +151 µs, as before. On both builds the `ui_render` guard's own one-label target (25 µs) missed in every run (+26.1 to +31.8 µs) and its scrolling limit (35 µs) in two runs of four, as they do at these loads.

### The reload hitch

**What lands without a hitch.** The image loader makes what the upload needs off the main thread (`PreparedUpload`):
- for an image over 256 px, its mip levels on every load, kept with it; and its pixels with their 8 px border only on a reload, so a first load holds no second copy of the pixels (its first draw borders them on the main thread, about 2.3 ms at 2048 px);
- for a smaller one, its bordered pixels and the pages' levels on the workers. One loaded with `load_now` has its levels made when it is first drawn, as before: made in the loader, they would cost the same thread the same time, and `scene_start` would be about 4.5 ms slower.

The renderer takes the upload the first time it writes that version, and frees it once written. A large reload with no texture to draw meanwhile drops its bordered level.

A new version of an image that already has a texture, whose upload is more than a frame's budget (2 MiB), is written into a new texture a band of rows a frame, level by level, while the old texture draws. The first image being written gets at least a quarter of the budget a frame whatever else the frame wrote, so it always ends; another starts, and makes its texture, only when the budget left holds one of its rows. Once the new texture is whole, it takes the old one's place at the start of the next frame, before anything is drawn. A 2048x2048 image's 22.7 MB take 11 frames, and are drawn in the 12th. A first upload, one within the budget (every image of 256 px or less, so the SVG), and an image changed in code are written at once.

**The guard** (`benches/assets.rs`, `reload_guard`) runs a 1280x720 `HeadlessEngine` drawing a 2048x2048 PNG (at a quarter of its size) and a 256 px SVG. Both come from a `MemorySource` with hot reload on. Over 2,000 frames, every 50th frame inserts a changed PNG and a changed SVG. Each frame's main-thread time is the wall time of the whole step (pump, update, UI pass and render step), with the GPU finished before each step. A reload's hitch is the slowest frame from the one its new value lands in to the one its new pixels are first drawn in. The guard holds each image's median hitch to within 0.5 ms of the median ordinary frame (one outside every reload). It also fails a reload whose upload did not begin in the frame it landed in, and one not drawn by the end of the run. It counts the frames more than 1.1 ms over an ordinary frame, outside reloads and in them.

Two things about how it is paced:
- The frames run at 60 Hz. Back to back (under 100 µs a frame), the PNG takes longer to decode than 50 frames take, so each change overtook the last and the PNG never landed.
- The wait between frames spins. When the thread slept through it instead, an ordinary frame took 716 µs instead of 122 µs (the thread ran on a slower core).

Every change landed: 39 PNG and 39 SVG reloads per run. Each began its upload in the frame it landed in, and each was drawn within the run.

| Round (load) | Ordinary frame (render step), 95th percentile | PNG: slowest frame of each reload, median and max; landing frame | SVG: its frame, median and max | The frame after drawing: median, max |
|---|---|---|---|---|
| 1 (1.23) | 110.0 µs (98.1), +148.4 | +356.2 µs, +854.6; +317.4 | +183.5 µs, +370.3 | +48.7 µs, +272.4 |
| 2 (2.35) | 105.7 µs (94.9), +103.3 | +344.9 µs, +690.6; +308.2 | +168.4 µs, +273.8 | +44.6 µs, +290.6 |
| 3 (2.72) | 106.7 µs (95.7), +159.4 | +332.8 µs, +454.5; +313.1 | +167.0 µs, +314.0 | +42.9 µs, +218.7 |
| 4 (2.07) | 107.5 µs (96.3), +117.3 | +313.0 µs, +532.3; +301.7 | +149.9 µs, +320.9 | +46.9 µs, +179.0 |

Each PNG reload was drawn 12 frames after it landed (the landing frame counted), and each SVG reload in the frame it landed in. The PNG's medians hold with about 145 µs to spare. Three more runs of the branch alone read the PNG at +316, +334 and +328 µs and the SVG at +157 µs.

No frame was more than 1.1 ms over an ordinary one in 19 runs of the guard (1,493 ordinary and 507 reload frames each), 12 of them timing the landing frame's parts: making the new texture, 28 to 43 µs; its first band (wgpu clears the level first), about 224 µs and at most 302; the submit, about 57 µs and at most 430. On a busier machine (loads up to 4), 15 runs had three reloads with a frame over 1.1 ms (+1.5, +2.7 and +12.8 ms) in 585; the one caught frame by frame was a landing frame's render step.

**Before** (each reload written in the frame it lands in), the same guard reads the PNG at +4,061 µs median (max +4,688) and the SVG at +494.8 µs (max +716.6), at load 3.26.

**The budget.** A write costs the main thread about 85 to 110 µs a MB, with about 50 µs more for the first write after a submit (`Queue::write_texture`, measured alone). Two rounds of the guard at each budget (loads 2.3 to 4.0), the PNG's median hitch:

| Budget | Frames to drawing | Round 1 | Round 2 |
|---|---|---|---|
| 1 MiB | 23 | +261 µs | +289 µs |
| 2 MiB | 12 | +332 µs | +359 µs |
| 3 MiB | 9 | +446 µs | +435 µs |
| 4 MiB | 7 | +520 µs (missed) | +497 µs |

2 MiB keeps about 150 µs below the target for the machine's noise (ordinary frames' 95th percentile is 100 to 180 µs over their median), and writes a 2048 px image in 11 frames (0.18 s at 60 Hz). 1 MiB doubles the frames for little more margin.

**Memory per 2048x2048 image.**
- The pixels, 16.8 MB, as before.
- The levels below the first, 5.7 MB, kept with the image for its life, as before. A texture freed for room (the 64-texture cap; nothing frees one for going undrawn) and drawn again then needs only the border made on the main thread (about 2.3 ms), not its levels (19 to 29 ms).
- The bordered first level, 17.0 MB, only for a reload, from its load until the swap (12 frames when the image is drawn). A first load, and a reload with no texture to draw meanwhile, keep none.
- On the GPU, 22.7 MB; while a reload is written, 45.4 MB (both textures) for 11 frames.

Other designs:
- Taking the levels as well, and keeping nothing: the image keeps no more than its pixels, but a texture freed and drawn again makes its levels on the main thread, a 19 to 29 ms hitch.
- Keeping the bordered level for the image's life: 22.7 MB more than its pixels, for its life.

**Limits.**
- A large image's first upload is written in the frame it first draws, with its pixels bordered on the main thread: about 4 ms at 2048 px (2.3 ms of border, then the copy). That includes one first loaded with `load` during play, in the frame it lands.
- The UI lays an image it draws at its natural size out at the image's new size from the frame it lands. A large reload that changes the image's size draws its old pixels stretched into the new rect until the new texture takes over. Text icons and bitmap-font sheets that change size are drawn from the new size's regions of the old texture until then.
- Should the frame a large reload lands in go undrawn for long enough that its `Modified` event is missed, the reload starts from the next draw instead; in that one frame a resized image draws its old pixels at its new size.

### The shader reload

**What lands without a hitch.** Metal compiles a pipeline in the call that makes it, 70 to 130 ms for WGSL it has not compiled before. A shader already drawn with a pipeline the GPU accepted keeps drawing it when it is saved: its new version's module and pipelines are made on a thread of the renderer's own (the sprite batch's, or the effect stage's) through the shared `Device`, inside a validation error scope popped there, and the new version takes over at the start of the frame after they come back (shaders module notes). The loader's naga compile is kept with the shader, so the renderer runs naga on a loaded file no more; wgpu still parses the composed WGSL when it makes the module, on that thread. A first compile, and every compile on the web, is made at once on the main thread as before.

**The guard** (`benches/assets.rs`, `shader_reload_guard`) runs a 1280x720 `HeadlessEngine` drawing a 256 px sprite with a custom sprite shader and running a custom effect in each stage (display and HDR), all three from a `MemorySource` with hot reload on. Over 2,000 frames at 60 Hz, every 25th frame saves the next of the three as WGSL with a constant no save had before, so neither the engine's shared sources nor the system's shader cache knows it: a text saved before costs 1.6 to 5.9 ms instead, a cache hit. A reload's hitch is the slowest frame from the one its new version lands in to the first one drawn with it (`HeadlessEngine::shader_reload_pending`); the guard holds each shader's median to within 0.5 ms of the median ordinary frame.

**Before** (each new version compiled on the main thread in the frame it landed in, the sprite drawn plain and the effect skipped in that frame), at load 7.4: the landing frame, median +90.8 ms for the sprite shader (max +96.6), +72.5 ms for the display effect (max +80.6) and +71.8 ms for the HDR effect (max +84.1).

**After**, four runs of the guard (the campaign below, loads 3.6 to 4.0):

| Run | Ordinary frame | Sprite shader: slowest frame of each reload, median and max; landing frame | Display effect | HDR effect |
|---|---|---|---|---|
| 1 | 343.4 µs | +104.4 µs, +229.3; +15.0 | +80.7 µs, +326.0; +11.6 | +79.8 µs, +341.5; +32.3 |
| 2 | 330.8 µs | +105.1 µs, +176.6; +0.0 | +88.9 µs, +385.6; +15.0 | +91.4 µs, +138.4; +13.3 |
| 3 | 321.4 µs | +123.4 µs, +161.9; +19.8 | +95.6 µs, +304.4; +11.7 | +97.8 µs, +183.3; +24.5 |
| 4 | 326.5 µs | +136.2 µs, +294.5; +24.4 | +103.9 µs, +1,535.5; -32.2 | +98.8 µs, +199.5; +32.1 |

- Every save landed and was drawn: 27 sprite and 26 of each effect's reloads a run. The sprite shader was drawn 7 frames after it landed (the landing frame counted; at most 10), each effect 6.
- Frames more than 1.1 ms over an ordinary one: 1 of about 500 reload frames in the four runs (run 4's display effect), and 0 to 2 of about 1,500 ordinary frames a run (one +32 ms).
- The hand-play harness, saving new WGSL each time (debug build, 60 Hz): `reload shader`'s two good saves took 133.2 and 99.2 ms before, and their slowest frames 0.70 and 0.65 ms after, against a 0.49 ms ordinary frame. `reload effect`'s took 113.8 and 76.7 ms before (the landing frame ran no effect), and 0.66 and 0.62 ms after (0.53 ms ordinary); the effect ran in every frame, the new one from 5 to 7 frames after landing.

**Limits.**
- A shader first drawn during play still compiles in the frame it first draws, on the main thread, and draws plain (an effect is skipped) until its pipeline's check comes back a frame later. So does a blend mode a shader is first drawn with.
- On the web there are no threads: a saved shader compiles in the next frame it draws, as before.

### Physics sort cost

`sync_colliders` sorts its changed work list every step. The physics bench's compound case is the typical-size scene with each of its 1,000 bodies made of four 20 px square colliders on child entities, so each step moves 4,000 colliders. Three rounds each way, against the build before the registry:

| Bench | Before | After | Change |
|---|---|---|---|
| 1000 bouncing bodies | 463.9 µs (450.8, 465.9, 463.9) | 453.5 µs (453.5, 455.2, 449.7) | -2.2% |
| 1000 bouncing 40-50 px bodies | 363.6 µs (361.4, 369.2, 363.6) | 366.5 µs (366.5, 363.8, 368.3) | +0.8% |
| 1000 compound bodies of 4 colliders | 1,078.2 µs (1,074.2, 1,132.0, 1,078.2) | 1,056.3 µs (1,070.5, 1,056.3, 1,051.6) | -2.0% |
| 100 characters on tiles | 573.6 µs (572.7, 589.8, 573.6) | 576.7 µs (573.8, 586.7, 576.7) | +0.5% |
| empty scene | 87.9 µs (87.9, 145.3, 77.5) | 85.6 µs (85.6, 85.9, 80.6) | -2.6% |

Round 2's "before" run ended at load 9.95 (its compound and empty scene are high). The sort costs nothing measurable, with 4,000 colliders moving every step.

### The pump with values waiting

`next_settled` and `find_cycle` grow with the number of waiting values. `pump_waiting` (`benches/assets.rs`) loads N UI files that each import one file whose read is held, so all N wait on it. It reports the median of 1,000 pumps with nothing to settle, and of the 50 pumps that each take in one more parent. Over eleven runs:

| Waiting | A still pump | A pump taking in one more |
|---|---|---|
| 10 | 0.08 to 0.12 µs | 0.46 to 1.04 µs |
| 100 | 0.46 to 0.62 µs | 1.00 to 2.00 µs |
| 1,000 | 4.79 to 6.08 µs | 6.88 to 10.75 µs |

Both grow linearly, about 6 ns per waiting value per pump: a still pump scans every waiting value, and a new parent's cycle check looks each value up by a scan. A thousand values waiting cost about 6 µs a frame. Taking a thousand parents in, one a pump, costs about 5 ms in all, spread over those frames.

### The e2e ten-frame bound

`tests/e2e/assets.rs`'s `a_load_from_update_is_loaded_within_ten_frames` holds under load: on the measured code it passed in four runs at loads 3.3 to 3.5 (1.1 to 2.0 s each), and in every workspace test run. It is left as it is.
