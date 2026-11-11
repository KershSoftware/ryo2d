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
