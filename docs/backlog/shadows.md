# Shadows: speed

## Measured

What is measured: a frame over the 1024 x 1024 cave map (`benches/tilemap.rs`'s: 2,378 occluder chains) with shadowed lights, by `cargo bench --bench lighting`: 1920x1080 headless at zoom 1, ambient night, the median of 120 frames after 30 of warm-up, a frame being the whole step and the wait for the GPU to finish it. The cases: `one_light_over_cave` is one shadowed 300 px light following a camera that flies a loop over the map, minus the same frame with no light; `sixty_four_lights_quarter_visible` is 64 shadowed 200 px lights on a fixed 8 x 8 grid (480 x 270 px apart) over the middle of the map, the still camera seeing 16; `two_hundred_fifty_six_lights_in_view` is 256 shadowed 120 px lights on a 16 x 16 grid inside the still camera's view.

Machine: Apple M5 Pro, 18 cores, shared with other work.

### Before

The CPU shadow volumes (`src/core/lighting/shadow.rs`).

One-minute load before each run (`uptime`), every one above 3 and so flagged: run 1 4.28, run 2 4.28, run 3 4.02 (the busiest processes at the third check were WindowServer, a shell, Warp and system daemons, each under 17% of a core). Nothing waited for a quiet machine. A first run of the same bench, made at load 4.94 before a tidy-up of the bench's imports, read 537.7 (unlit), 693.2 (one light), 1,138.1 and 2,416.2 µs; it is not in the table.

Medians in microseconds:

| Bench | Run 1 | Run 2 | Run 3 | Median of runs | Load |
|-------|-------|-------|-------|----------------|------|
| `one_light_over_cave` | 194.5 | 149.2 | 187.0 | 187.0 | 4.28, 4.28, 4.02 (flagged) |
| `sixty_four_lights_quarter_visible` | 1,108.7 | 1,161.3 | 1,177.2 | 1,161.3 | 4.28, 4.28, 4.02 (flagged) |
| `two_hundred_fifty_six_lights_in_view` | 2,364.6 | 3,012.8 | 2,453.0 | 2,453.0 | 4.28, 4.28, 4.02 (flagged) |

`one_light_over_cave` is the difference of two frames: the cave with no light, 529.2, 537.4 and 549.9 µs (median 537.4), and with the one light, 723.6, 686.5 and 737.0 µs (median 723.6). The 256 lights' second run (3,012.8) is far from its neighbours; the load did not mark it.
