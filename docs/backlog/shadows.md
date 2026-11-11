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
| `sixty_four_lights_quarter_visible` | 1,306.9 | 1,363.1 | 922.2 | 905.4 | 915.2 | 915.2 | at most 326.7 (4x faster than Before's median; superseded by the lights' own guard below) | missed in every run (superseded) | 6.76, 6.76, 6.54 (flagged) |
| `two_hundred_fifty_six_lights_in_view` | 2,664.6 | 2,684.2 | 1,420.0 | 1,477.0 | 1,410.5 | 1,420.0 | under 16,600, and no slower than Before's slowest, 2,684.2 | met in every run | 6.76, 6.76, 6.54 (flagged) |

`one_light_over_cave` is the difference of two frames: the cave with no light, 615.2, 615.5 and 594.5 µs, and with the one light, 773.2, 782.7 and 813.8 µs.

`sixty_four_lights_quarter_visible` is also measured as the lights' own cost: the frame with the 64 lights over the same still camera's frame with none (`cave_still_unlit`), as `sixty_four_lights_quarter_visible_lights`. The 4x target is held to that, since the cave's own drawing (about 540 µs) is in both frames and is not shadow work: Before it was 1,306.9 less 590.9 = 716.0 µs, so the guard is at most 179.0 µs. Before's baseline (590.9) was the flying camera's unlit cave (`cave_unlit`, the only empty frame Before measured), while After subtracts the still camera's empty frame (`cave_still_unlit`); the two read within about 10 µs of each other (the performance round's runs below read them side by side: 527.3, 531.8 and 531.1 flying against 534.3, 530.8 and 533.3 still), so the choice moves the guard by under 3 µs. A second set of three runs, loads 3.74, 3.68 and 3.68 (flagged, above 3):

| Bench | Before | Run 1 | Run 2 | Run 3 | After median | Guard | Verdict |
|-------|--------|-------|-------|-------|--------------|-------|---------|
| `sixty_four_lights_quarter_visible_lights` | 716.0 | 273.6 | 363.5 | 261.8 | 273.6 (spread 101.7, 37.2%) | at most 179.0 | missed in every run |

The whole frames read 814.6, 914.8 and 806.0 µs (median 814.6, not guarded) over empty frames of 541.0, 551.3 and 544.3 µs. In that set `one_light_over_cave` read 160.5, 163.9 and 170.3 and `two_hundred_fifty_six_lights_in_view` 1,276.6, 1,234.3 and 1,255.8, both met. Before the performance round below, the spec's 4x target for 64 lights was not met: the lights cost 2.6x less than before (716.0 to 273.6 µs), not 4x; after it, 4.75x. Where the rest went is measured in the performance round below.

#### Performance round

Where the lights' cost went, measured before changing anything with temporary, uncommitted timing (the shadow planner's and the atlas draw's CPU time on the clock, and the frame with and without each part: the same still scene lit with no lights, with the 64 lights unshadowed, with only the lights that reach the view, and with the atlas pass skipped), loads 4.7 to 6.9 (flagged; each comparison is within one run). Of the 64 lights, 24 reach the view (a 200 px light reaches past the 16 whose centres it shows) and get slots (96 rows) and 673 edge-views, about 28 a light; the other 40 cost nothing (no slot, no edges, no instance drawn: the scene with only the 24 measured the same). Most of the cost was not shadow work: turning lighting on at all (clearing the light buffer, copying the 1920 x 1080 scene image for the compose pass, composing) cost 114 µs with no light, the 24 lights' own drawing 60 to 77 µs, and the shadows 40 to 55 µs, of which the planner was 10 µs of CPU, the atlas pass about 35 µs (7 µs of CPU recording it, the rest the GPU's pass) and the shadow look-ups the rest. Nothing else was redone per frame: the index does no work in a static frame, and the plan's vectors, the uniforms and the bind groups are kept.

Three changes removed the waste. A camera drawing straight into the scene image is composed from the scene image itself into the lit image (it never wrote over what it read, so its copy was waste; a camera lit in place keeps its copy): lighting on fell from 114 to 54 µs. The atlas is drawn only when the plan's edges differ from those it holds (compared byte for byte, or when its texture is new), so a static scene's frame records no atlas pass: the shadows fell from 40 to 55 µs to 21 to 28 µs. A light pixel the light does not reach (its quad's corners) no longer reads the atlas (no measurable change in this scene; it keeps soft lights' look-ups off pixels they cannot light). The planner still runs every frame (10 µs here), which is what lets any change to a light, an occluder, a floor or a camera redraw the atlas without a change list; caching it would need floors and lifts tracked too, for 10 µs.

| Lights' cost over the empty frame (µs, one run each) | Before the round | After |
|---|---|---|
| Lighting on, no light (light buffer, scene copy, compose) | 114 | 54 |
| 64 lights drawn, unshadowed (24 reach the view) | 60 to 77 | 69 to 72 |
| Shadows (planner 10 µs CPU, atlas pass, look-ups) | 40 to 55 | 21 to 28 |
| Total, same runs | about 230 | about 152 |

Three runs of `cargo bench --bench lighting` after the changes, each log opening with its `uptime`; the one-minute load read 2.64, 2.38 and 2.27, under 3, so none is flagged (the load was checked for about four minutes after the builds before the first run):

| Bench | Before | Run 1 | Run 2 | Run 3 | After median (spread) | Guard | Verdict |
|-------|--------|-------|-------|-------|-----------------------|-------|---------|
| `sixty_four_lights_quarter_visible_lights` | 716.0 | 150.7 | 150.1 | 151.2 | 150.7 (1.1, 0.7%) | at most 179.0 | met in every run |
| `one_light_over_cave` | 203.2 (slowest 211.2) | 84.1 | 87.3 | 82.5 | 84.1 (4.8, 5.7%) | at most 211.2 | met in every run |
| `two_hundred_fifty_six_lights_in_view` | 2,664.6 (slowest 2,684.2) | 1,055.5 | 1,078.1 | 1,058.7 | 1,058.7 (22.6, 2.1%) | under 16,600 and 2,684.2 | met in every run |

The 64 lights' whole frames read 685.0, 680.9 and 684.6 µs over still empty frames of 534.3, 530.8 and 533.3; the flying unlit cave read 527.3, 531.8 and 531.1, with one light 611.4, 619.2 and 613.6. Verdict: the lights cost 4.75x less than Before (716.0 to 150.7 µs), so the spec's 4x target is met, with 28 µs to spare. The floor under it is lighting itself: the same lights with no shadows would cost about 125 µs (lighting on and their drawing), so shadows are now about a sixth of the lights' cost in this scene.

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
