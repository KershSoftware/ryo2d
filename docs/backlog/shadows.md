# Shadows: speed

## Measured

What is measured: a frame over the 1024 x 1024 cave map (`benches/tilemap.rs`'s: 2,378 occluder chains) with shadowed lights, by `cargo bench --bench lighting`: 1920x1080 headless at zoom 1, ambient night, the median of 120 frames after 30 of warm-up, a frame being the whole step and the wait for the GPU to finish it. The cases: `one_light_over_cave` is one shadowed 300 px light following a camera that flies a loop over the map, minus the same frame with no light; `sixty_four_lights_quarter_visible` is 64 shadowed 200 px lights on a fixed 8 x 8 grid (480 x 270 px apart) over the middle of the map, the still camera seeing 16; `two_hundred_fifty_six_lights_in_view` is 256 shadowed 120 px lights on a 16 x 16 grid inside the still camera's view.

Machine: Apple M5 Pro, 18 cores, shared with other work.

### Before

The CPU shadow volumes (`src/core/lighting/shadow.rs`).

Three runs of `cargo bench --bench lighting`, each log opening with its `uptime` (run immediately before the bench). The one-minute load read 3.74 before every one of them (the three readings were identical: the clock had not moved between them), above 3 and so flagged; the busiest processes at the earlier check were WindowServer, a shell, Warp and system daemons, each under 17% of a core. Nothing waited for a quiet machine.

Medians in microseconds, the spread being the slowest run minus the fastest (and as a share of the median of runs):

| Bench | Run 1 | Run 2 | Run 3 | Median of runs | Slowest run | Spread |
|-------|-------|-------|-------|----------------|-------------|--------|
| `one_light_over_cave` | 203.2 | 180.6 | 211.2 | 203.2 | 211.2 | 30.6 (15.1%) |
| `sixty_four_lights_quarter_visible` | 1,306.9 | 1,263.9 | 1,363.1 | 1,306.9 | 1,363.1 | 99.2 (7.6%) |
| `two_hundred_fifty_six_lights_in_view` | 2,684.2 | 2,664.6 | 2,484.5 | 2,664.6 | 2,684.2 | 199.7 (7.5%) |

`one_light_over_cave` is the difference of two frames: the cave with no light, 590.9, 596.0 and 589.4 µs (median 590.9, spread 6.6, 1.1%), and with the one light, 794.1, 776.6 and 800.6 µs (median 794.1, spread 24.0, 3.0%).

Later guards compare against this bench's own figures: the slowest run for "no slower than before" checks, the median of runs for the 4x target.

An earlier set of runs of the same bench read, in the same order, 194.5, 149.2 and 187.0 µs for `one_light_over_cave` (median 187.0), 1,108.7, 1,161.3 and 1,177.2 for the 64 lights (1,161.3) and 2,364.6, 3,012.8 and 2,453.0 for the 256 (2,453.0), at loads of 4.28, 4.28 and 4.02; their loads were not all logged with the runs, and they are superseded by the table above (the machine's frame times moved by about 10% between the sets: the empty-cave frame read 537.4 then and 590.9 now).

### Why this is not the 30 µs figure

`docs/backlog/tilemap.md` and the tilemap spec speak of about 30 µs for a shadowed light. That is a different measure. `benches/tilemap.rs` takes the lit map's frame over an empty lit frame, so the light is in both frames and only the map's occluder cost is left, with a 500 px light and a camera flying at about 560 px/s. `one_light_over_cave` subtracts the unlit map: it holds the whole lighting pass (the light buffer, the compose, the light itself and its shadow volumes), with a 300 px light on another path. Side by side, from one run of `cargo bench --bench tilemap -- static_frame` (load 3.50, flagged): the lit map over the empty lit frame, median 75.0 µs (95th percentile 195.2), an empty lit frame 151.3 µs against an empty frame 106.2 µs. The guards of the shadow rewrite compare against the Before figures above, not against the 30 µs.

### After

The per-light GPU shadow maps (one light draw per camera, the occluder index). The same bench, the same scenes, light sizes and counts as Before; every bench light is hard, as the Before lights were. Three runs of `cargo bench --bench lighting`, each log opening with its `uptime`; the one-minute load read 6.76, 6.76 and 6.54 (the runs came within the same few seconds), above 3 and so flagged on every run. The busiest processes at the check before the runs were other work on the machine (two scripted game runs and a Python script at about 50% of a core each, WindowServer at 47%). Nothing waited for a quiet machine.

Medians in microseconds:

| Bench | Before median | Before slowest | Run 1 | Run 2 | Run 3 | After median | Guard | Verdict | Load (runs 1, 2, 3) |
|-------|---------------|----------------|-------|-------|-------|--------------|-------|---------|---------------------|
| `one_light_over_cave` | 203.2 | 211.2 | 158.0 | 167.2 | 219.3 | 167.2 | no slower than Before's slowest, 211.2 | met in runs 1 and 2, missed in run 3 (219.3) | 6.76, 6.76, 6.54 (flagged) |
| `sixty_four_lights_quarter_visible` | 1,306.9 | 1,363.1 | 922.2 | 905.4 | 915.2 | 915.2 | at most 326.7 (4x faster than Before's median) | missed in every run | 6.76, 6.76, 6.54 (flagged) |
| `two_hundred_fifty_six_lights_in_view` | 2,664.6 | 2,684.2 | 1,420.0 | 1,477.0 | 1,410.5 | 1,420.0 | under 16,600, and no slower than Before's slowest, 2,684.2 | met in every run | 6.76, 6.76, 6.54 (flagged) |

`one_light_over_cave` is the difference of two frames: the cave with no light, 615.2, 615.5 and 594.5 µs, and with the one light, 773.2, 782.7 and 813.8 µs.

`sixty_four_lights_quarter_visible` is also measured as the lights' own cost: the frame with the 64 lights over the same still camera's frame with none (`cave_still_unlit`), as `sixty_four_lights_quarter_visible_lights`. The 4x target is held to that, since the cave's own drawing (about 540 µs) is in both frames and is not shadow work: Before it was 1,306.9 less 590.9 = 716.0 µs, so the guard is at most 179.0 µs. A second set of three runs, loads 3.74, 3.68 and 3.68 (flagged, above 3):

| Bench | Before | Run 1 | Run 2 | Run 3 | After median | Guard | Verdict |
|-------|--------|-------|-------|-------|--------------|-------|---------|
| `sixty_four_lights_quarter_visible_lights` | 716.0 | 273.6 | 363.5 | 261.8 | 273.6 | at most 179.0 | missed in every run |

The whole frames read 814.6, 914.8 and 806.0 µs (median 814.6, not guarded) over empty frames of 541.0, 551.3 and 544.3 µs. In that set `one_light_over_cave` read 160.5, 163.9 and 170.3 and `two_hundred_fifty_six_lights_in_view` 1,276.6, 1,234.3 and 1,255.8, both met. The spec's 4x target for 64 lights is not met: the lights cost 2.6x less than before (716.0 to 273.6 µs), not 4x. Where the rest goes was not measured (no per-phase timings were taken, so the split between the CPU planner and the GPU passes is unknown).

## Limits

- Edge quality: edges are crisp for lights up to 512 px radius at zoom 1 at High (half that at Medium, twice at Ultra); beyond, edges soften and step.
- At most 512 shadowed lights a frame: the nearest are kept, with one warning.
- A light inside a tile map's wall lights nothing outside it (map walls are open chains): put wall lights at the wall's face.
- One light image per camera: a light brightens every storey under it.
- No directional (sun) light and no adaptive per-light resolution.
- Soft shadows: at Low, wide penumbrae show faint stair-step bands, and a thin post's soft edge can end in a step that changes as the light moves; a near occluder's penumbra narrows when a farther occluder (still nearer than the pixel) is in its search arc.
- Only exactly shared sides of closed occluders are cancelled: partly overlapping sides still cast, and a keyhole polygon's slit edges cast.
- A back face tilted more than about 56 degrees from the light's ray can darken itself slightly through the filter.
- An edge too small to cover a texel centre writes nothing (tiny far occluders).
