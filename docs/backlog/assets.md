# Asset Registry (Sub-project 5a)

Spec: `docs/specs/2026-10-23-asset-registry-design.md`.

## 5a: measured

Measured on an Apple M5 Pro (Metal), on a shared machine (a browser, Discord and an editor open). Medians move with the machine's load, so every run's 1-minute load is given.

**Method.** The build before the registry (2ba51866: the old `AssetManager`, with its own `benches/assets.rs`) and the branch ran in four alternated rounds; the side that ran first flipped each round. Each run waited until the 1-minute load was 4 or less and no other process was drawing on the GPU. Loads at the runs' starts were 2.3 to 4.0.
- Each side ran its own `benches/assets.rs`, `benches/ui_render.rs` and `benches/foundation.rs`. The last two are the same on both sides, but for the image API.
- The numbers are criterion's medians, and the guards' medians from their own output.

### The targets

| Target | Before | After | Verdict |
|---|---|---|---|
| Scene start, `load_now`: 40 images and 3 UI files | 6,223 µs (rounds 6,122, 6,505, 6,210, 6,237) | 5,722 µs (5,740, 5,772, 5,645, 5,705) | no slower: **met**, -8.1% (rounds -6.2, -11.3, -9.1, -8.5%) |
| Scene start, `load` and the workers, the same assets | 6,223 µs (synchronous) | 3,131 µs (3,122, 3,128, 3,171, 3,134) | faster: **met**, -49.7% |
| Empty frame, render step (`ui_render` guard) | 70.5 µs (rounds 66.4 to 73.6) | 68.9 µs (67.5 to 75.1) | within 3%: **met**, -2.3% |
| Idle settings screen, render step (`ui_render` guard) | 70.8 µs (67.2 to 74.5) | 69.2 µs (65.9 to 74.2) | within 3%: **met**, -2.3% |
| Empty frame, whole step (`ui_render`'s `empty_frame`) | 190.8 µs | 192.4 µs | within 3%: **met**, +0.8% |
| Idle settings screen, whole step (`settings_idle`) | 188.9 µs | 186.6 µs | within 3%: **met**, -1.2% |
| 10,000 sprites, whole step (`foundation`) | 1,086.8 µs | 1,037.2 µs | within 3%: **met**, -4.6% |
| Reload hitch: a 2048x2048 PNG | +4.0 to +4.2 ms, written in the frame it lands | +362 to +376 µs, the slowest frame of each reload until it is drawn (12 frames) | within 0.5 ms: **met** |
| Reload hitch: a 256 px SVG | +419 to +499 µs, made and written in the frame it lands | +129 to +184 µs, written in the frame it lands | within 0.5 ms: **met** |

The fonts (`scene_start_fonts`, branch only: the build before had no public way to make a `Fonts` registry) take the same set to 5,772 µs (5,855, 5,777, 5,680, 5,767), 5 to 116 µs over the round's `scene_start`. `scene_start_async`'s workers have already started (an untimed one-pixel load), as a game's have by its second scene; starting them was not measured. The workers now make each image's upload (its bordered pixels and mip levels) as they load it, about 1.4 ms of the 3.1 ms; the frame that first draws those images then makes none of it.

### Scene start (`benches/assets.rs`)

| Bench | Round 1 | Round 2 | Round 3 | Round 4 |
|---|---|---|---|---|
| before, `scene_start` | 6,122 µs (load 3.83) | 6,505 µs (3.05) | 6,210 µs (3.30) | 6,237 µs (3.38) |
| after, `scene_start` | 5,740 µs (3.95) | 5,772 µs (2.49) | 5,645 µs (3.11) | 5,705 µs (3.38) |
| after, `scene_start_fonts` | 5,855 µs | 5,777 µs | 5,680 µs | 5,767 µs |
| after, `scene_start_async` | 3,122 µs | 3,128 µs | 3,171 µs | 3,134 µs |

### Idle and empty frames (`benches/ui_render.rs` and `benches/foundation.rs`, four rounds)

The `ui_render` guard reads the render step: seven engines stepped in turn, with the GPU idle before each step. The values are medians of the four rounds, with the range of the rounds.

| Frame | Before | After | Change |
|---|---|---|---|
| empty frame, render step | 70.5 µs (66.4 to 73.6) | 68.9 µs (67.5 to 75.1) | -2.3% |
| idle screen, render step | 70.8 µs (67.2 to 74.5) | 69.2 µs (65.9 to 74.2) | -2.3% |
| one label, over the empty frame | +24.5 to +31.4 µs | +22.0 to +25.8 µs | |
| scrolling, over the empty frame | +31.8 to +39.2 µs | +28.4 to +35.1 µs | |

The guard's own targets for one label (25 µs) and scrolling (35 µs) were missed in rounds 1, 2 and 4 by the build before (one label +25.8, +26.5, +31.4; scrolling +39.2) and in rounds 1 and 4 by the branch (one label +25.7, +25.8; scrolling +35.1). Both builds miss them at loads near 4; they are not the registry's targets.

Criterion's medians of the same four rounds:

| Bench | Before | After | Change |
|---|---|---|---|
| `empty_frame` | 190.8 µs | 192.4 µs | +0.8% |
| `settings_idle` | 188.9 µs | 186.6 µs | -1.2% |
| `settings_idle_unretained` | 180.1 µs | 180.3 µs | +0.1% |
| headless frame, 10k sprites | 1,086.8 µs | 1,037.2 µs | -4.6% |
| propagate 10k, every root moved | 74.6 µs | 74.8 µs | +0.3% |
| propagate 10k, nothing moved | 8.2 µs (6.3 to 8.5) | 7.5 µs (7.2 to 9.5) | -8.6% |

`settings_label` and `settings_scrolling` are left out: stepped back to back they are paced by the GPU, and read one of two values a run on both builds (`settings_scrolling` about 142 or about 225 µs).

The 10k-sprite frame looks up each sprite's place in the atlas (`TextureAtlas::lookup` and `texture`, through `SpriteGpuRenderer::slot`). Once the atlas grew, those calls stopped being inlined, and the frame took about 40 µs (3.8%) longer than with the build just before. Marked `#[inline]`, the frame is faster than that build's.

Propagation ran the same source on both sides. Its 8 µs "nothing moved" case swings from 6.3 to 9.5 µs between rounds of one build, so its change is noise, not the registry.

### The reload hitch

**What lands without a hitch.** The image loader makes what the upload needs off the main thread: the pixels with their 8 px border, and their mip levels (`PreparedUpload`). It makes them on the workers for every image, and under `load_now` for an image over 256 px, whose levels it would make anyway. A small image loaded with `load_now` has its levels made when it is first drawn, as before: made in the loader, they would cost the same thread the same time, and `scene_start` would be about 4.5 ms slower. The renderer takes the upload the first time it writes that version, and frees it once written.

A new version of an image that already has a texture, whose upload is more than a frame's budget (2 MiB), is written into a new texture a band of rows a frame, level by level, while the old texture draws. Once it is whole, it takes the old one's place at the start of the next frame, before anything is drawn. A 2048x2048 image's 22.7 MB take 11 frames, and are drawn in the 12th. A first upload, one within the budget (every image of 256 px or less, so the SVG), and an image changed in code are written at once.

**The guard** (`benches/assets.rs`, `reload_guard`) runs a 1280x720 `HeadlessEngine` drawing a 2048x2048 PNG (at a quarter of its size) and a 256 px SVG. Both come from a `MemorySource` with hot reload on. Over 2,000 frames, every 50th frame inserts a changed PNG and a changed SVG. Each frame's main-thread time is the wall time of the whole step (pump, update, UI pass and render step), with the GPU finished before each step. A reload's hitch is the slowest frame from the one its new value lands in to the one its new pixels are first drawn in. The guard holds each image's median hitch to within 0.5 ms of the median ordinary frame (one outside every reload). It also fails a reload whose upload did not begin in the frame it landed in, and one not drawn by the end of the run.

Two things about how it is paced:
- The frames run at 60 Hz. Back to back (under 100 µs a frame), the PNG takes longer to decode than 50 frames take, so each change overtook the last and the PNG never landed.
- The wait between frames spins. When the thread slept through it instead, an ordinary frame took 716 µs instead of 122 µs (the thread ran on a slower core).

Every change landed: 39 PNG and 39 SVG reloads per run. Each began its upload in the frame it landed in, and each was drawn within the run.

| Round (load) | Ordinary frame (render step), 95th percentile | PNG: slowest frame of each reload, median and max; landing frame | SVG: its frame, median and max | The frame after drawing: median, max |
|---|---|---|---|---|
| 1 (3.95) | 193.8 µs (153.7), +146.8 | +363.9 µs, +12,792; +317.5 | +184.2 µs, +297.5 | +30.7 µs, +199.2 |
| 2 (2.49) | 192.0 µs (151.8), +159.5 | +361.6 µs, +695.5; +323.5 | +160.7 µs, +306.7 | +50.0 µs, +196.3 |
| 3 (3.11) | 192.1 µs (151.9), +166.9 | +365.8 µs, +762.8; +325.0 | +165.0 µs, +299.2 | +55.7 µs, +220.0 |
| 4 (3.38) | 287.9 µs (219.5), +183.0 | +376.2 µs, +1,007; +316.8 | +128.8 µs, +669.0 | +20.6 µs, +513.6 |

Each PNG reload was drawn 12 frames after it landed (the landing frame counted), and each SVG reload in the frame it landed in. The PNG's medians hold with about 125 µs to spare.

The slowest PNG frames are rare. Over 15 runs of the guard at this budget (585 PNG reloads), three reloads had a frame over 1.1 ms: +1.5 ms, +2.7 ms and +12.8 ms. The one caught frame by frame was the render step of a landing frame, the second reload of its run; that reload's other frames were all under +310 µs. It fits the GPU memory for the second texture being found the first time it is needed, not the bands.

The same guard, run once on the code before this (each reload written in the frame it landed in): the PNG +4,061 µs median (max +4,688), the SVG +494.8 µs (max +716.6), load 3.26.

**The budget.** A write costs the main thread about 85 to 110 µs a MB, with about 50 µs more for the first write after a submit (`Queue::write_texture`, measured alone). Two rounds of the guard at each budget (loads 2.3 to 4.0), the PNG's median hitch:

| Budget | Frames to drawing | Round 1 | Round 2 |
|---|---|---|---|
| 1 MiB | 23 | +261 µs | +289 µs |
| 2 MiB | 12 | +332 µs | +359 µs |
| 3 MiB | 9 | +446 µs | +435 µs |
| 4 MiB | 7 | +520 µs (missed) | +497 µs |

2 MiB keeps about 150 µs below the target for the machine's noise (ordinary frames' 95th percentile is 130 to 180 µs over their median), and writes a 2048 px image in 11 frames (0.18 s at 60 Hz). 1 MiB doubles the frames for little more margin.

**Memory per 2048x2048 image.**
- The pixels, 16.8 MB, as before.
- The levels below the first, 5.7 MB, kept with the image for its life, as before. A texture freed unused (300 frames undrawn, or the 64-texture cap) and drawn again then needs only the border made on the main thread (about 2.3 ms), not its levels (19 to 29 ms).
- The bordered first level, 17.0 MB, from the load until the renderer writes it: the first upload, or the swap of a reload. An image loaded and never drawn keeps it until it is freed.
- On the GPU, 22.7 MB; while a reload is written, 45.4 MB (both textures) for 11 frames.

Other designs:
- Taking the levels as well, and keeping nothing: the image keeps no more than its pixels, but a texture freed and drawn again makes its levels on the main thread, a 19 to 29 ms hitch.
- Keeping the bordered level for the image's life: 22.7 MB more than its pixels, for its life.

**Limits.**
- A large image first loaded with `load` during play is written in the frame it lands, with no border or levels to make: about 2 ms for a 2048 px image.
- The UI lays an image it draws at its natural size out at the image's new size from the frame it lands. A large reload that changes the image's size draws its old pixels stretched into the new rect until the new texture takes over.
- Should the frame a large reload lands in go undrawn for long enough that its `Modified` event is missed, the reload starts from the next draw instead; in that one frame a resized image draws its old pixels at its new size.

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

`next_settled` and `find_cycle` grow with the number of waiting values. `pump_waiting` (`benches/assets.rs`) loads N UI files that each import one file whose read is held, so all N wait on it. It reports the median of 1,000 pumps with nothing to settle, and of the 50 pumps that each take in one more parent. Over seven runs:

| Waiting | A still pump | A pump taking in one more |
|---|---|---|
| 10 | 0.08 to 0.12 µs | 0.46 to 1.04 µs |
| 100 | 0.46 to 0.62 µs | 1.00 to 2.00 µs |
| 1,000 | 4.79 to 6.08 µs | 6.88 to 10.42 µs |

Both grow linearly, about 6 ns per waiting value per pump: a still pump scans every waiting value, and a new parent's cycle check looks each value up by a scan. A thousand values waiting cost about 6 µs a frame. Taking a thousand parents in, one a pump, costs about 5 ms in all, spread over those frames.

### The e2e ten-frame bound

`tests/e2e/assets.rs`'s `a_load_from_update_is_loaded_within_ten_frames` held in every run made: six measurement rounds, each right after its round's benches (1.0 to 1.9 s), and every `cargo test --workspace` since. It is left as it is.
