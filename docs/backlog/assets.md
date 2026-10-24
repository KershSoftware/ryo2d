# Asset Registry (Sub-project 5a)

Spec: `docs/specs/2026-10-23-asset-registry-design.md`.

## 5a: measured

Measured on an Apple M5 Pro (Metal), on a shared machine (a browser, Discord and an editor open). Medians move with the machine's load, so every run's 1-minute load is given.

**Method.** The build before the registry (2ba51866, Task 1: the old `AssetManager`, with its own `benches/assets.rs`) and the branch (62bd931d with these benches) ran in alternated rounds. The side that ran first flipped each round. Each run waited until the 1-minute load was 4 or less and no other process was drawing on the GPU. A first attempt was stopped when another agent's game tests (three game windows drawing on the GPU) started; none ran during the runs below. Load at the runs' starts was 2.4 to 4.0, and at their ends up to 4.6, but one run (round 2's "before" physics) ended at 9.95.
- Each side ran its own `benches/assets.rs`, `benches/ui_render.rs` and `benches/foundation.rs`. The last two are the same on both sides, but for the image API.
- Both ran the branch's `benches/physics.rs`, its compound case copied into Task 1's tree (the file is otherwise unchanged since then).
- The numbers are criterion's medians, and the guards' medians from their own output.

### The targets

| Target | Before | After | Verdict |
|---|---|---|---|
| Scene start, `load_now`: 40 images and 3 UI files | 6,234 µs (rounds 6,234, 6,309, 5,914) | 5,669 µs (5,669, 5,873, 5,514) | no slower: **met**, -9.1% (rounds -9.1, -6.9, -6.8%) |
| Scene start, `load` and the workers, the same assets | 6,234 µs (synchronous) | 1,720 µs (1,720, 1,728, 1,695) | faster: **met**, -72% (rounds -72.4, -72.6, -71.3%) |
| Empty frame, render step (`ui_render` guard) | 64.5 µs (six rounds 62.3 to 72.9) | 65.1 µs (63.7 to 66.5) | within 3%: **met**, +0.9% |
| Idle settings screen, render step (`ui_render` guard) | 65.2 µs (60.8 to 73.3) | 63.9 µs (62.8 to 67.0) | within 3%: **met**, -2.0% |
| Empty frame, whole step (`ui_render`'s `empty_frame`) | 187.8 µs | 187.1 µs | within 3%: **met**, -0.4% |
| Idle settings screen, whole step (`settings_idle`) | 185.6 µs | 184.9 µs | within 3%: **met**, -0.4% |
| 10,000 sprites, whole step (`foundation`) | 1,060.6 µs | 1,046.5 µs | within 3%: **met**, -1.3% |
| Reload hitch: a 2048x2048 PNG | | landing frame +4.01 to +4.24 ms over an ordinary frame | within 0.5 ms: **missed** |
| Reload hitch: a 256 px SVG | | landing frame +419 to +490 µs; slowest landing +676 to +1,111 µs | within 0.5 ms: met by the medians, narrowly; its slowest landings are over |

The fonts (`scene_start_fonts`, branch only: the build before had no public way to make a `Fonts` registry) take the same set to 5,684 µs (5,684, 5,919, 5,494). The two fonts add between -20 and +46 µs to the round's `scene_start`. `scene_start_async`'s workers have already started (an untimed one-pixel load), as a game's have by its second scene; starting them was not measured.

### Scene start (`benches/assets.rs`)

| Bench | Round 1 | Round 2 | Round 3 |
|---|---|---|---|
| before, `scene_start` | 6,234 µs (load 3.05) | 6,309 µs (3.88) | 5,914 µs (3.79) |
| after, `scene_start` | 5,669 µs (2.89) | 5,873 µs (3.17) | 5,514 µs (3.36) |
| after, `scene_start_fonts` | 5,684 µs | 5,919 µs | 5,494 µs |
| after, `scene_start_async` | 1,720 µs | 1,728 µs | 1,695 µs |

### Idle and empty frames (`benches/ui_render.rs` and `benches/foundation.rs`, six rounds)

The `ui_render` guard reads the render step: seven engines stepped in turn, with the GPU idle before each step. It reads the UI pass from each retained engine stepped alone. The values are medians of the six rounds, with the range of the rounds.

| Frame | Before | After | Change |
|---|---|---|---|
| empty frame, render step | 64.5 µs (62.3 to 72.9) | 65.1 µs (63.7 to 66.5) | +0.9% |
| idle screen, render step | 65.2 µs (60.8 to 73.3) | 63.9 µs (62.8 to 67.0) | -2.0% |
| idle screen, over the empty frame | -1.5 to +1.0 µs | -1.3 to +0.5 µs | |
| one label, over the empty frame | +19.7 to +28.2 µs | +21.4 to +24.1 µs | |
| scrolling, over the empty frame | +24.7 to +38.7 µs | +27.3 to +31.0 µs | |
| idle UI pass, stepped alone | 15.6 µs | 15.2 µs | -2.6% |
| empty UI pass, stepped in turn | 3.0 µs | 2.7 µs | |

The before build's guard missed its own one-label target (25 µs) in rounds 1 and 5: +26.5 and +28.2 µs, with every render step high (empty 66.6 and 72.9 µs). The branch's guard met every target in every round.

Criterion's medians of the same six rounds:

| Bench | Before | After | Change |
|---|---|---|---|
| `empty_frame` | 187.8 µs | 187.1 µs | -0.4% |
| `settings_idle` | 185.6 µs | 184.9 µs | -0.4% |
| `settings_idle_unretained` | 178.2 µs | 177.7 µs | -0.3% |
| `settings_scrolling` | 140.6 µs | 137.8 µs | -2.0% |
| headless frame, 10k sprites | 1,060.6 µs | 1,046.5 µs | -1.3% |
| propagate 10k, every root moved | 74.2 µs | 73.6 µs | -0.8% |
| propagate 10k, nothing moved | 7.8 µs (6.8 to 8.2) | 8.0 µs (5.5 to 8.7) | +3.7% |

`settings_label` is left out: stepped back to back it is paced by the GPU and reads about 128 or about 220 µs a run, on both builds (before 127.4 to 226.3, after 128.6 to 223.8).

Propagation ran the same source on both sides: `src/core/ecs/propagate.rs`, `transform.rs`, `Cargo.toml` and `Cargo.lock` have not changed since Task 1. Its 8 µs "nothing moved" case swings from 5.5 to 8.7 µs between rounds of one build, so its +3.7% is noise, not the registry.

### The reload hitch (the miss)

The guard (`benches/assets.rs`, `reload_guard`) runs a 1280x720 `HeadlessEngine` drawing a 2048x2048 PNG (a quarter of its size) and a 256 px SVG. Both come from a `MemorySource` with hot reload on. Over 2,000 frames, every 50th frame inserts a changed PNG and a changed SVG. Each frame's main-thread time is the wall time of the whole step (pump, update, UI pass and render step), with the GPU finished before each step. The guard holds each image's median landing frame to within 0.5 ms of an ordinary frame's median.

Two things about how it is paced:
- The frames run at 60 Hz. Back to back (under 100 µs a frame), the PNG takes longer to decode than 50 frames take, so each change overtook the last and the PNG never landed.
- The wait between frames spins. When the thread slept through it instead, an ordinary frame took 716 µs instead of 122 µs (the thread ran on a slower core), and the PNG landed at +4.4 ms, the SVG at +783 µs.

Every change landed: 39 PNG and 39 SVG landings per run, never in the same frame. Each landing uploaded in its own frame.

| Round (load) | Ordinary frame (render step) | PNG landing: median, max | SVG landing: median, max | The frame after: median, max |
|---|---|---|---|---|
| trial (3.22) | 121.8 µs (105.2) | +4,007 µs, +4,820 | +499 µs, +638 | +50 µs, +739 |
| 1 (2.89) | 155.8 µs (128.4) | +4,242 µs, +5,472 | +490 µs, +676 | +45 µs, +426 |
| 2 (3.17) | 306.9 µs (237.7) | +4,245 µs, +4,754 | +419 µs, +1,111 | +19 µs, +229 |
| 3 (3.36) | 152.9 µs (128.2) | +4,008 µs, +4,849 | +474 µs, +858 | +38 µs, +861 |

**Profile.** xctrace Time Profiler of one guard run: 2,000 frames, 39 landings of each image, main-thread samples inside the guard's steps, warm-up and engine start left out, 1 ms samples. Almost all of the PNG's 4 ms is the upload, in `TextureAtlas::slot` (`src/core/graphics/atlas.rs`): about 4.3 ms of main-thread time per landing. The pump's part is under 0.1 ms per landing.
- `extrude_border`: about 2.05 ms per landing. The 2048x2048 image is copied, texel by texel, into a 2064x2064 buffer with an 8 px border, on the main thread. The loader already made this bordered buffer on its own thread, to make the mips from (`standalone_mips`), but kept only the levels below it.
- `Queue::write_texture`: about 2.1 ms per landing, nearly all `memmove`. wgpu copies the 17 MB first level and the loader's 5.6 MB of mips into its staging memory, on the main thread.
- Making a new standalone texture each reload (`resident` frees the old one first): about 0.08 ms.
- The SVG goes on a shared atlas page, whose four mip levels `mip_chain` makes at upload, on the main thread (only images over 256 px get their mips on the loader thread). Its bordering and upload are small. At 1 ms samples, its 39 landings are too few to split further.

Levers, not taken (the owner decides):
- Keep the loader's bordered first level, so the upload borders nothing: about 2 ms.
- Make an atlas image's mips on the loader thread too: most of the SVG's 0.45 ms.
- Copy the pixels into a staging buffer on the loader thread, or spread a big upload over several frames. The 2.1 ms `write_texture` copy stays without one of these, so the first two levers alone still miss for the PNG.

The traces were left in the session's scratchpad (`t14/reload_guard.trace`, and `t14/reload_hitch.trace` of the criterion bench).

### Physics sort cost (ruling 2)

`sync_colliders` now sorts its changed work list every step. The physics bench had no case with many compound bodies, so one was added: the typical-size scene with each of its 1,000 bodies made of four 20 px square colliders on child entities, so each step moves 4,000 colliders. Three rounds each way:

| Bench | Before | After | Change |
|---|---|---|---|
| 1000 bouncing bodies | 463.9 µs (450.8, 465.9, 463.9) | 453.5 µs (453.5, 455.2, 449.7) | -2.2% |
| 1000 bouncing 40-50 px bodies | 363.6 µs (361.4, 369.2, 363.6) | 366.5 µs (366.5, 363.8, 368.3) | +0.8% |
| 1000 compound bodies of 4 colliders | 1,078.2 µs (1,074.2, 1,132.0, 1,078.2) | 1,056.3 µs (1,070.5, 1,056.3, 1,051.6) | -2.0% |
| 100 characters on tiles | 573.6 µs (572.7, 589.8, 573.6) | 576.7 µs (573.8, 586.7, 576.7) | +0.5% |
| empty scene | 87.9 µs (87.9, 145.3, 77.5) | 85.6 µs (85.6, 85.9, 80.6) | -2.6% |

Round 2's "before" run ended at load 9.95 (its compound and empty scene are high). The sort costs nothing measurable, with 4,000 colliders moving every step.

### The pump with values waiting (ruling 3)

`next_settled` and `find_cycle` grow with the number of waiting values. This was measured with N UI files each importing one file whose read is held, so all N wait on it (`pump_waiting` in `benches/assets.rs`). It reports the median of 1,000 pumps with nothing to settle, and of the 50 pumps that each take in one more parent, in three runs:

| Waiting | A still pump | A pump taking in one more |
|---|---|---|
| 10 | 0.08 to 0.12 µs | 0.83 to 1.04 µs |
| 100 | 0.54 to 0.62 µs | 1.67 to 2.00 µs |
| 1,000 | 5.50 to 6.08 µs | 8.04 to 10.42 µs |

Both grow linearly, about 6 ns per waiting value per pump: a still pump scans every waiting value, and a new parent's cycle check looks each value up by a scan. A thousand values waiting cost about 6 µs a frame. Taking a thousand parents in, one a pump, costs about 5 ms in all, spread over those frames.

### The e2e ten-frame bound (ruling 4)

`tests/e2e/assets.rs`'s `a_load_from_update_is_loaded_within_ten_frames` held in every run made: six measurement rounds, run straight after each round's benches (1.0 to 1.9 s each), and the gate's `cargo test --workspace`. It is left as it is.
